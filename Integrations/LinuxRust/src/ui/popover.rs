//! Quick View popover (mac `MenuCardView` order / Qt `qml/QuickView.qml` layout):
//! header, provider switcher, Overview or provider card body, footer.

use crate::model::{CostEntry, Entry, QuotaWindow};
use crate::settings::Settings;
use crate::state::{App, Message, Page};
use iced::widget::{
    button, column, container, horizontal_rule, progress_bar, row, scrollable, text, Column, Row,
    Space, Stack,
};
use iced::{Alignment, Background, Border, Color, Element, Font, Length, Theme};

const EM_DASH: &str = "—";
const WARNING_RGB: (u8, u8, u8) = (0xd9, 0x87, 0x4a);
const ERROR_RGB: (u8, u8, u8) = (0xcf, 0x74, 0x43);
const DEFAULT_METER_RGB: (u8, u8, u8) = (0xc9, 0x87, 0x57);

fn rgb(c: (u8, u8, u8)) -> Color {
    Color::from_rgb8(c.0, c.1, c.2)
}

/// Secondary text on the app's forced dark theme (Qt: primary at 0.65 alpha).
fn muted() -> Color {
    Color::from_rgba(1.0, 1.0, 1.0, 0.65)
}

fn semibold() -> Font {
    Font {
        weight: iced::font::Weight::Semibold,
        ..Font::DEFAULT
    }
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
    format!("${v:.2}")
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
    } else if !app.updated.is_empty() {
        format!("Updated {}", app.updated)
    } else {
        "Waiting for usage…".to_string()
    }
}

fn format_credits(v: f64) -> String {
    if v.fract() == 0.0 && v.is_finite() && v.abs() <= i64::MAX as f64 {
        format!("{}", v as i64)
    } else {
        format!("{v:.2}")
    }
}

pub fn view(app: &App) -> Element<Message> {
    let body = body(app);
    column![
        header(app),
        switcher(app),
        horizontal_rule(1),
        scrollable(body).width(Length::Fill).height(Length::Fill),
        horizontal_rule(1),
        footer(),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn header(app: &App) -> Element<'_, Message> {
    let refresh = button(text("Refresh").size(12))
        .on_press_maybe(if app.busy {
            None
        } else {
            Some(Message::Refresh)
        })
        .style(|theme: &Theme, status: button::Status| {
            let base = button::text(theme, status);
            button::Style {
                border: Border::default().rounded(6.0),
                ..base
            }
        })
        .padding([2, 8]);
    let title = row![
        text("CodexBar")
            .size(14)
            .font(semibold())
            .width(Length::Fill),
        refresh,
    ]
    .align_y(Alignment::Center);
    container(column![title, text(updated_line(app)).size(12).color(muted())].spacing(2))
        .padding(iced::Padding {
            top: 12.0,
            right: 14.0,
            bottom: 8.0,
            left: 20.0,
        })
        .width(Length::Fill)
        .into()
}

/// Capsule meter: iced progress bar with optional pace-expected tick overlay.
fn meter_bar(
    display_value: i64,
    fill: Color,
    tick: Option<f32>,
    height: f32,
) -> Element<'static, Message> {
    let radius = height / 2.0;
    let bar = progress_bar(0.0..=100.0, meter_fraction(display_value) * 100.0)
        .height(height)
        .width(Length::Fill)
        .style(move |theme: &Theme| {
            let track = theme.extended_palette().background.strong.color;
            progress_bar::Style {
                background: Background::Color(track),
                bar: Background::Color(fill),
                border: Border::default().rounded(radius),
            }
        });
    match tick {
        None => bar.into(),
        Some(f) => {
            let left = (f.clamp(0.0, 1.0) * 1000.0).round() as u16;
            let mut tick_row = Row::new().height(Length::Fixed(height));
            if left > 0 {
                tick_row = tick_row.push(Space::with_width(Length::FillPortion(left)));
            }
            tick_row = tick_row.push(
                container(Space::with_width(Length::Fixed(2.0)))
                    .width(Length::Fixed(2.0))
                    .height(Length::Fill)
                    .style(|_: &Theme| container::Style {
                        background: Some(Background::Color(Color::WHITE)),
                        ..Default::default()
                    }),
            );
            if left < 1000 {
                tick_row = tick_row.push(Space::with_width(Length::FillPortion(1000 - left)));
            }
            Stack::with_children(vec![Element::from(bar), tick_row.into()])
                .width(Length::Fill)
                .height(Length::Fixed(height))
                .into()
        }
    }
}

fn tab_style(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme: &Theme, status: button::Status| {
        let palette = theme.extended_palette();
        let (background, text_color) = if selected {
            (
                Some(Background::Color(palette.primary.strong.color)),
                Color::WHITE,
            )
        } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
            (
                Some(Background::Color(palette.background.weak.color)),
                palette.background.base.text,
            )
        } else {
            (None, palette.background.base.text)
        };
        button::Style {
            background,
            text_color,
            border: Border::default().rounded(9.0),
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
    .center_y(Length::Fixed(20.0))
    .into()
}

fn switcher(app: &App) -> Element<'_, Message> {
    let mut tabs = Row::new().spacing(3).padding(iced::Padding {
        top: 0.0,
        right: 16.0,
        bottom: 0.0,
        left: 16.0,
    });
    // Overview tab
    let overview_selected = app.selected == -1;
    let overview_color = if overview_selected {
        Color::WHITE
    } else {
        muted()
    };
    tabs = tabs.push(
        button(
            column![
                overview_glyph(overview_color),
                text("Overview").size(11).color(overview_color),
                Space::with_height(Length::Fixed(3.0)),
            ]
            .align_x(Alignment::Center)
            .spacing(2)
            .width(Length::Fixed(64.0)),
        )
        .on_press(Message::SelectEntry(-1))
        .style(tab_style(overview_selected))
        .padding([6, 4]),
    );
    for (i, entry) in app.entries.iter().enumerate() {
        let selected = app.selected == i as i32;
        let fg = if selected { Color::WHITE } else { muted() };
        let name = crate::model::provider_name(&entry.provider);
        let mini: Element<Message> = match entry.windows.first() {
            Some(w) => {
                let value = quota_display_value(w.remaining, &app.settings.quota_display);
                let fill = if selected {
                    Color::WHITE
                } else {
                    rgb(accent_rgb(&entry.provider))
                };
                container(meter_bar(value, fill, None, 3.0))
                    .width(Length::Fixed(52.0))
                    .into()
            }
            None => text(EM_DASH).size(11).color(fg).into(),
        };
        tabs = tabs.push(
            button(
                column![
                    text(crate::descriptors::tag(&entry.provider))
                        .size(12)
                        .font(semibold())
                        .color(fg),
                    text(name).size(11).color(fg),
                    mini,
                ]
                .align_x(Alignment::Center)
                .spacing(2)
                .width(Length::Fixed(64.0)),
            )
            .on_press(Message::SelectEntry(i as i32))
            .style(tab_style(selected))
            .padding([6, 4]),
        );
    }
    scrollable(tabs)
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::new(),
        ))
        .width(Length::Fill)
        .height(Length::Fixed(69.0))
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
    let mut col = Column::new().spacing(12);
    col = col.push(text("Overview").size(20).font(semibold()));
    if !app.spending.is_empty() {
        let today: f64 = app.spending.iter().map(|c| c.today).sum();
        let month: f64 = app.spending.iter().map(|c| c.month).sum();
        let tokens: i64 = app.spending.iter().map(|c| c.tokens).sum();
        col = col.push(
            text(format!(
                "Today {}  ·  Last 30 days {}  ·  {} tokens",
                format_money(today),
                format_money(month),
                format_count(tokens)
            ))
            .size(12)
            .color(muted()),
        );
    } else if app.cost_busy {
        col = col.push(text("Reading local history…").size(12).color(muted()));
    }
    for (i, entry) in app.entries.iter().enumerate() {
        let name = crate::model::provider_name(&entry.provider);
        let (quota_text, meter) = match entry.windows.first() {
            Some(w) => {
                let value = quota_display_value(w.remaining, &app.settings.quota_display);
                let fill = if is_warning(
                    app.settings.warning_colors,
                    w.remaining,
                    app.settings.notify_threshold,
                ) {
                    rgb(WARNING_RGB)
                } else {
                    rgb(accent_rgb(&entry.provider))
                };
                (
                    format!(
                        "{value}% {}",
                        quota_display_suffix(&app.settings.quota_display)
                    ),
                    meter_bar(value, fill, None, 5.0),
                )
            }
            None => (
                EM_DASH.to_string(),
                meter_bar(0, rgb(accent_rgb(&entry.provider)), None, 5.0),
            ),
        };
        let mut lines = Column::new().spacing(6).width(Length::Fill);
        lines = lines.push(
            row![
                text(name).size(14).font(semibold()).width(Length::Fill),
                text(quota_text).size(12).color(muted()),
            ]
            .align_y(Alignment::Center),
        );
        if entry.windows.first().is_some() {
            lines = lines.push(meter);
        }
        col = col.push(
            button(container(lines).padding(11).width(Length::Fill))
                .on_press(Message::SelectEntry(i as i32))
                .style(|theme: &Theme, status: button::Status| {
                    let palette = theme.extended_palette();
                    let bg = if matches!(status, button::Status::Hovered | button::Status::Pressed)
                    {
                        palette.background.strong.color
                    } else {
                        palette.background.weak.color
                    };
                    button::Style {
                        background: Some(Background::Color(bg)),
                        text_color: palette.background.base.text,
                        border: Border::default().rounded(10.0),
                        ..Default::default()
                    }
                })
                .padding(0)
                .width(Length::Fill),
        );
    }
    container(col).padding(20).width(Length::Fill).into()
}

fn footer() -> Element<'static, Message> {
    let action = |label: &'static str, msg: Message| {
        button(text(label).size(13).width(Length::Fill))
            .on_press(msg)
            .style(|theme: &Theme, status: button::Status| {
                let base = button::text(theme, status);
                button::Style {
                    border: Border::default().rounded(6.0),
                    ..base
                }
            })
            .padding([6, 10])
            .width(Length::Fill)
    };
    let about = container(
        text(format!("About CodexBar (v{})", env!("CARGO_PKG_VERSION")))
            .size(13)
            .color(muted()),
    )
    .padding([6, 10])
    .width(Length::Fill);
    container(
        column![
            action("Usage & Spend…", Message::ShowPage(Page::Usage)),
            action("Settings…", Message::ShowPage(Page::Settings)),
            about,
            action("Quit", Message::Quit),
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

fn section_title(label: &str) -> Element<'static, Message> {
    text(label.to_string()).size(15).font(semibold()).into()
}

fn provider_body<'a>(
    app: &'a App,
    entry: &'a Entry,
    cost: Option<&'a CostEntry>,
) -> Element<'a, Message> {
    let s = &app.settings;
    let mut col = Column::new().spacing(14).width(Length::Fill);
    // Card header: name + account (line 1), updated + plan (line 2)
    let mut head1 = Row::new().align_y(Alignment::Center);
    head1 = head1.push(
        text(crate::model::provider_name(&entry.provider))
            .size(21)
            .font(semibold())
            .width(Length::Fill),
    );
    if !entry.account_label.is_empty() {
        head1 = head1.push(text(entry.account_label.clone()).size(12).color(muted()));
    }
    col = col.push(head1);
    let mut head2 = Row::new().align_y(Alignment::Center);
    head2 = head2.push(
        text(updated_line(app))
            .size(12)
            .color(muted())
            .width(Length::Fill),
    );
    if !entry.plan.is_empty() {
        head2 = head2.push(text(entry.plan.clone()).size(12).color(muted()));
    }
    col = col.push(head2);
    if !entry.error.is_empty() {
        col = col.push(text(entry.error.clone()).size(12).color(rgb(ERROR_RGB)));
    } else if !entry.status.is_empty() {
        col = col.push(text(entry.status.clone()).size(12).color(muted()));
    }
    col = col.push(horizontal_rule(1));
    // Metrics in order
    if entry.windows.is_empty() {
        col = col.push(text(EM_DASH).size(13).color(muted()));
    } else {
        for window in &entry.windows {
            col = col.push(metric_view(window, s, &entry.provider));
        }
    }
    // DeepSeek-only price clock
    if entry.provider == "deepseek" {
        col = col.push(deepseek_section());
    }
    // Credits
    if let Some(credits) = entry.credits {
        let mut sec = Column::new().spacing(6).width(Length::Fill);
        sec = sec.push(horizontal_rule(1));
        sec = sec.push(section_title("Credits"));
        sec = sec.push(
            text(format!("{} remaining", format_credits(credits)))
                .size(13)
                .color(muted()),
        );
        col = col.push(sec);
    }
    // Reset credits (hide at 0: model cannot distinguish unknown from none)
    if entry.reset_credits_available > 0 {
        let mut sec = Column::new().spacing(6).width(Length::Fill);
        sec = sec.push(horizontal_rule(1));
        sec = sec.push(section_title("Reset credits"));
        let mut line = format!("{} available", entry.reset_credits_available);
        if !entry.reset_credits_next_expiry.is_empty() {
            line.push_str(&format!(
                " · Next expires {}",
                entry.reset_credits_next_expiry
            ));
        }
        sec = sec.push(text(line).size(13).color(muted()));
        col = col.push(sec);
    }
    // Extra usage / provider cost
    if !entry.extra_usage_text.is_empty() {
        let mut sec = Column::new().spacing(6).width(Length::Fill);
        sec = sec.push(horizontal_rule(1));
        sec = sec.push(section_title("Extra usage"));
        sec = sec.push(text(entry.extra_usage_text.clone()).size(13).color(muted()));
        col = col.push(sec);
    }
    // Cost (local history)
    if s.show_costs && (cost.is_some() || app.cost_busy || !app.cost_error.is_empty()) {
        col = col.push(cost_section(app, cost));
    }
    // Provider detail sections
    for section in &entry.details {
        let mut sec = Column::new().spacing(6).width(Length::Fill);
        sec = sec.push(horizontal_rule(1));
        if !section.title.is_empty() {
            sec = sec.push(text(section.title.clone()).size(14).font(semibold()));
        }
        for (label, value, secondary) in &section.rows {
            let shown_value = if value.is_empty() {
                EM_DASH
            } else {
                value.as_str()
            };
            let mut line = if label.is_empty() {
                shown_value.to_string()
            } else {
                format!("{label}: {shown_value}")
            };
            if !secondary.is_empty() {
                line.push_str(&format!(" · {secondary}"));
            }
            sec = sec.push(text(line).size(12).color(muted()));
        }
        col = col.push(sec);
    }
    container(col).padding(20).width(Length::Fill).into()
}

fn metric_view(window: &QuotaWindow, s: &Settings, provider: &str) -> Element<'static, Message> {
    let amount = quota_display_value(window.remaining, &s.quota_display);
    let warning = is_warning(s.warning_colors, window.remaining, s.notify_threshold);
    let fill = if warning {
        rgb(WARNING_RGB)
    } else {
        rgb(accent_rgb(provider))
    };
    let tick = pace_tick_fraction(s.show_pace, window.pace_expected, &s.quota_display);
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
            text(label).size(15).font(semibold()).width(Length::Fill),
            text(format!(
                "{amount}% {}",
                quota_display_suffix(&s.quota_display)
            ))
            .size(13),
        ]
        .align_y(Alignment::Center),
    );
    col = col.push(meter_bar(amount, fill, tick, 6.0));
    col = col.push(text(reset).size(12).color(muted()));
    if s.show_pace && !window.pace.is_empty() {
        col = col.push(text(window.pace.clone()).size(12).color(muted()));
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
    let mut sec = Column::new().spacing(6).width(Length::Fill);
    sec = sec.push(horizontal_rule(1));
    sec = sec.push(section_title("Peak Price Clock"));
    sec = sec.push(
        text(if clock.badge.is_empty() {
            EM_DASH.to_string()
        } else {
            clock.badge.clone()
        })
        .size(10)
        .font(semibold())
        .color(badge_color),
    );
    sec = sec.push(
        text(if clock.countdown.is_empty() {
            EM_DASH.to_string()
        } else {
            clock.countdown.clone()
        })
        .size(24)
        .font(semibold()),
    );
    if !clock.next_transition.is_empty() {
        sec = sec.push(text(clock.next_transition.clone()).size(12).color(muted()));
    }
    for (tag, range, is_peak) in &clock.windows {
        let tag_color = if *is_peak {
            Color::from_rgb(0.91, 0.42, 0.36)
        } else {
            Color::from_rgb(0.22, 0.74, 0.48)
        };
        sec = sec.push(
            row![
                text(tag.clone())
                    .size(12)
                    .font(semibold())
                    .color(tag_color)
                    .width(Length::Fixed(64.0)),
                text(range.clone()).size(11),
            ]
            .align_y(Alignment::Center),
        );
    }
    sec = sec.push(
        text("Excludes Chinese public holidays")
            .size(9)
            .color(muted()),
    );
    sec.into()
}

fn cost_section(app: &App, cost: Option<&CostEntry>) -> Element<'static, Message> {
    let mut sec = Column::new().spacing(6).width(Length::Fill);
    sec = sec.push(horizontal_rule(1));
    sec = sec.push(section_title("Cost across accounts"));
    if app.cost_busy {
        sec = sec.push(text("Reading local history…").size(12).color(muted()));
    } else if !app.cost_error.is_empty() {
        sec = sec.push(text(app.cost_error.clone()).size(12).color(rgb(ERROR_RGB)));
    }
    if let Some(cost) = cost {
        sec = sec.push(
            text(format!(
                "Today {}  ·  Last 30 days {}",
                format_money(cost.today),
                format_money(cost.month)
            ))
            .size(13),
        );
        sec = sec.push(
            text(format!(
                "{} tokens · {}",
                format_count(cost.tokens),
                provenance_label(&cost.provenance)
            ))
            .size(12)
            .color(muted()),
        );
        if !cost.error.is_empty() {
            sec = sec.push(text(cost.error.clone()).size(12).color(muted()));
        }
        if !cost.coverage.is_empty() {
            sec = sec.push(text(cost.coverage.clone()).size(12).color(muted()));
        }
    }
    sec.into()
}

#[cfg(test)]
mod tests {
    use super::*;

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
