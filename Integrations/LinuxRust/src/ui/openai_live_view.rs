//! OpenAI Live Usage window (mac `OpenAILiveUsageView`): model + range pickers, TPM tiles,
//! per-minute stacked chart, per-model table and the loading / empty / unauthorized / error
//! states. Org rate-limit utilization is not shown (not ported yet).

use crate::charts::{segment_color, BarChart};
use crate::openai_live::{self as live, LiveError, Snapshot};
use crate::share::compact_count;
use crate::state::{App, Message};
use iced::widget::{
    button, column, container, horizontal_rule, horizontal_space, pick_list, row, scrollable, text,
    Canvas, Column, Row, Space,
};
use iced::{Background, Border, Color, Element, Font, Length, Theme};

const CHART_KEY: &str = "openai-live";

fn muted() -> Color {
    Color::from_rgba(1.0, 1.0, 1.0, 0.65)
}

fn semibold() -> Font {
    Font {
        weight: iced::font::Weight::Semibold,
        ..Font::DEFAULT
    }
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn range_label(minutes: i64) -> &'static str {
    match minutes {
        15 => "15m",
        60 => "1h",
        _ => "6h",
    }
}

fn header(app: &App, names: &[String]) -> Element<'static, Message> {
    let st = &app.live;
    let mut options = vec![live::ALL_MODELS.to_string()];
    options.extend(names.iter().cloned());
    if let Some(f) = &st.filter {
        if !options.contains(f) {
            options.push(f.clone());
        }
    }
    let selected = st
        .filter
        .clone()
        .unwrap_or_else(|| live::ALL_MODELS.to_string());
    let picker = pick_list(options, Some(selected), |s| {
        Message::OpenAiLiveFilter((s != live::ALL_MODELS).then_some(s))
    })
    .width(Length::Fixed(240.0));
    let mut ranges = Row::new().spacing(0);
    for w in live::WINDOWS {
        let active = st.window_minutes == w;
        ranges = ranges.push(
            crate::ui::style::button(text(range_label(w)).size(13))
                .on_press(Message::OpenAiLiveRange(w))
                .padding([5, 12])
                .style(crate::ui::style::segment(active)),
        );
    }
    let updated = st
        .last_updated
        .map(|t| live::updated_text(t, now()))
        .unwrap_or_default();
    let refresh = crate::ui::style::button(
        text(if st.loading {
            "Refreshing…"
        } else {
            "Refresh"
        })
        .size(13),
    )
    .on_press_maybe((!st.loading).then_some(Message::OpenAiLiveRefresh))
    .padding([5, 12])
    .style(crate::ui::style::push);
    row![
        picker,
        horizontal_space(),
        ranges,
        text(updated).size(12).color(muted()),
        refresh,
    ]
    .spacing(12)
    .align_y(iced::Alignment::Center)
    .into()
}

fn error_row(app: &App) -> Option<Element<'static, Message>> {
    let err = app.live.error.as_ref()?;
    let msg = if *err == LiveError::RateLimited {
        let secs = app
            .live
            .next_refresh
            .map(|t| {
                t.saturating_duration_since(std::time::Instant::now())
                    .as_secs_f64()
                    .ceil() as u64
            })
            .unwrap_or(app.live.interval_secs);
        format!("Rate limited; retrying in {secs}s")
    } else {
        live::error_text(err)
    };
    let mut r = Row::new().spacing(10).align_y(iced::Alignment::Center);
    r = r.push(text(msg).size(13).color(muted()).width(Length::Fill));
    if *err == LiveError::MissingCredential {
        r = r.push(
            crate::ui::style::button(text("Open OpenAI Settings").size(13))
                .on_press(Message::OpenAiLiveOpenSettings)
                .padding([5, 12])
                .style(crate::ui::style::push),
        );
    }
    Some(
        container(r)
            .padding(10)
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(Color::from_rgba(0.81, 0.45, 0.26, 0.12))),
                border: Border::default().rounded(8.0),
                ..Default::default()
            })
            .into(),
    )
}

fn tile(title: String, value: String, detail: String) -> Element<'static, Message> {
    let mut c = column![
        text(title).size(12).color(muted()),
        text(value).size(22),
        text(detail).size(11).color(muted()),
    ]
    .spacing(3)
    .width(Length::FillPortion(1));
    c = c.spacing(3);
    c.into()
}

fn metrics(s: &Snapshot, filter: Option<&str>) -> Element<'static, Message> {
    let m = s.summary(filter);
    let clock = |t: Option<i64>| t.map(live::clock).unwrap_or_else(|| "—".into());
    let mut tokens_detail = format!(
        "Input {} · cached {} · output {}",
        compact_count(m.input_tokens),
        compact_count(m.cached_input_tokens),
        compact_count(m.output_tokens)
    );
    if m.batch_tokens > 0 {
        tokens_detail.push_str(&format!(" · Batch API {}", compact_count(m.batch_tokens)));
    }
    row![
        tile(
            m.latest_minute_start
                .map(|t| format!("Latest · {}", live::clock(t)))
                .unwrap_or_else(|| "Latest".into()),
            compact_count(m.latest_tpm),
            "TPM".into()
        ),
        tile(
            "5-min avg".into(),
            compact_count(m.five_minute_average_tpm as i64),
            "TPM".into()
        ),
        tile(
            "Peak".into(),
            compact_count(m.peak_tpm),
            format!("TPM · {}", clock(m.peak_minute_start))
        ),
        tile(
            "Tokens in window".into(),
            compact_count(m.total_tokens),
            tokens_detail
        ),
        tile(
            "Requests in window".into(),
            compact_count(m.requests),
            String::new()
        ),
    ]
    .spacing(12)
    .into()
}

fn chart(app: &App, s: &Snapshot) -> Element<'static, Message> {
    let filter = app.live.filter.as_deref();
    let (bars, legend) = live::chart_bars(s, filter);
    let hover = app
        .chart_hover
        .as_ref()
        .filter(|(k, _)| k == CHART_KEY)
        .map(|(_, i)| *i);
    let canvas: Element<Message> = Canvas::new(BarChart {
        key: CHART_KEY.into(),
        bars,
        color: segment_color(0),
        peak_cap: false,
        hover,
        fmt: live::tokens_fmt,
        axis: live::tokens_fmt,
        y_max: None,
    })
    .width(Length::Fill)
    .height(Length::Fixed(220.0))
    .into();
    let mut key = Row::new().spacing(14);
    for (name, idx) in legend {
        key = key.push(
            row![
                container(Space::new(8, 8)).style(move |_| container::Style {
                    background: Some(Background::Color(segment_color(idx))),
                    border: Border::default().rounded(2.0),
                    ..Default::default()
                }),
                text(name).size(11).color(muted()),
            ]
            .spacing(5)
            .align_y(iced::Alignment::Center),
        );
    }
    column![
        text("Tokens per minute").size(14).font(semibold()),
        canvas,
        key,
        text("Gray ticks are minutes OpenAI is still reporting (unknown, not zero).")
            .size(11)
            .color(muted()),
    ]
    .spacing(6)
    .into()
}

fn cell(t: String) -> Element<'static, Message> {
    text(t).size(13).width(Length::Fixed(90.0)).into()
}

fn table(app: &App, s: &Snapshot, names: &[String]) -> Element<'static, Message> {
    let head = |t: &'static str| -> Element<'static, Message> {
        text(t)
            .size(12)
            .color(muted())
            .width(Length::Fixed(90.0))
            .into()
    };
    let mut col = Column::new().spacing(2);
    col = col.push(
        row![
            text("Model").size(12).color(muted()).width(Length::Fill),
            head("Latest TPM"),
            head("Peak TPM"),
            head("Tokens"),
            head("Requests"),
        ]
        .spacing(8)
        .padding([0, 8]),
    );
    col = col.push(horizontal_rule(1));
    let mut rows: Vec<(String, Option<String>)> = vec![(live::ALL_MODELS.into(), None)];
    rows.extend(names.iter().map(|n| (n.clone(), Some(n.clone()))));
    for (label, filter) in rows {
        let m = s.summary(filter.as_deref());
        let selected = app.live.filter == filter;
        let line = row![
            text(label).size(13).width(Length::Fill),
            cell(compact_count(m.latest_tpm)),
            cell(compact_count(m.peak_tpm)),
            cell(compact_count(m.total_tokens)),
            cell(compact_count(m.requests)),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        col = col.push(
            crate::ui::style::button(line)
                .on_press(Message::OpenAiLiveFilter(filter))
                .width(Length::Fill)
                .padding(8)
                .style(move |theme: &Theme, status| {
                    let base = button::text(theme, status);
                    button::Style {
                        background: selected
                            .then(|| Background::Color(Color::from_rgba(0.35, 0.55, 0.94, 0.18))),
                        border: Border::default().rounded(6.0),
                        ..base
                    }
                }),
        );
    }
    col.into()
}

fn footer(project: Option<&str>) -> Element<'static, Message> {
    let line = |t: String| text(t).size(11).color(muted());
    let mut c = Column::new().spacing(3);
    c = c.push(line(
        "OpenAI Admin Usage API · 1-minute buckets · the newest minute can arrive late".into(),
    ));
    if let Some(p) = project {
        c = c.push(line(format!("Project: {p}")));
    }
    c = c.push(line(
        "Organization rate limits are not shown on Linux yet. The key comes from \
         Settings > Providers > OpenAI."
            .into(),
    ));
    c.width(Length::Fill).into()
}

pub fn view(app: &App) -> Element<Message> {
    let st = &app.live;
    let names = st
        .snapshot
        .as_ref()
        .map(|s| s.model_names())
        .unwrap_or_default();
    let mut body = Column::new().spacing(16).width(Length::Fill);
    body = body.push(header(app, &names));
    if let Some(e) = error_row(app) {
        body = body.push(e);
    }
    match &st.snapshot {
        Some(s) => {
            body = body.push(metrics(s, st.filter.as_deref()));
            body = body.push(chart(app, s));
            if s.summary(st.filter.as_deref()).total_tokens == 0 {
                body = body.push(
                    text("No API traffic in this window")
                        .size(13)
                        .color(muted()),
                );
            }
            body = body.push(table(app, s, &names));
            body = body.push(footer(s.project_id.as_deref()));
        }
        None => {
            let msg = if st.loading {
                Some("Loading live usage…")
            } else if st.error.is_none() {
                Some("No API traffic in this window")
            } else {
                None
            };
            if let Some(m) = msg {
                body = body.push(
                    container(text(m).size(15))
                        .width(Length::Fill)
                        .center_x(Length::Fill)
                        .padding(40),
                );
            }
            body = body.push(footer(None));
        }
    }
    container(
        scrollable(body.padding(iced::Padding {
            top: 0.0,
            right: 14.0,
            bottom: 0.0,
            left: 4.0,
        }))
        .height(Length::Fill),
    )
    .padding(20)
    .width(Length::Fill)
    .height(Length::Fill)
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(theme.palette().background)),
        ..Default::default()
    })
    .into()
}
