//! Share Stats window: live preview of the 1200x630 card plus Copy Image /
//! Copy Stats / Save PNG. Mirrors ShareStatsCardView.swift (layout, labels, order)
//! and ShareStatsWindowController.swift. The card is drawn at 1:1 at the top-left
//! of the window so a window screenshot cropped to `CARD_*` is the exported image.

use crate::share::{self, SharePayload};
use crate::state::{App, Message};
use iced::font::Weight;
use iced::widget::{column, container, horizontal_space, row, text, Space};
use iced::{Alignment, Background, Border, Color, Element, Font, Length};

pub const CARD_W: f32 = 1200.0;
pub const CARD_H: f32 = 630.0;
/// Window padding: the card's top-left corner in window logical pixels.
pub const PAD: f32 = 24.0;
pub const WINDOW_SIZE: (f32, f32) = (CARD_W + 2.0 * PAD, 800.0);

const ACCENT: Color = Color::from_rgb(0.93, 0.56, 0.36);

#[derive(Clone, Copy)]
struct Palette {
    bg: Color,
    fg: Color,
}

impl Palette {
    fn new(dark: bool) -> Self {
        if dark {
            Self {
                bg: Color::from_rgb8(0x17, 0x17, 0x17),
                fg: Color::WHITE,
            }
        } else {
            Self {
                bg: Color::WHITE,
                fg: Color::from_rgb8(0x1c, 0x1c, 0x1e),
            }
        }
    }
    fn secondary(self) -> Color {
        Color { a: 0.6, ..self.fg }
    }
    fn tint(self, a: f32) -> Color {
        Color { a, ..self.fg }
    }
}

fn font(weight: Weight) -> Font {
    Font {
        weight,
        ..Font::DEFAULT
    }
}

fn t<'a>(s: impl Into<String>, size: f32, weight: Weight, color: Color) -> iced::widget::Text<'a> {
    text(s.into())
        .size(size)
        .font(font(weight))
        .color(color)
        .line_height(iced::widget::text::LineHeight::Relative(if size > 30.0 {
            1.0
        } else {
            1.3
        }))
}

fn mark<'a>() -> Element<'a, Message> {
    let bar = |h: f32| {
        container(Space::new(5, h)).style(|_| container::Style {
            background: Some(Background::Color(ACCENT)),
            border: Border {
                radius: 3.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
    };
    container(
        row![bar(28.0 * 0.38), bar(28.0 * 0.68), bar(28.0)]
            .spacing(4)
            .align_y(Alignment::End),
    )
    .width(34)
    .height(34)
    .align_x(Alignment::Center)
    .align_y(Alignment::Center)
    .into()
}

fn header<'a>(pal: Palette) -> Element<'a, Message> {
    row![
        mark(),
        Space::with_width(14),
        t("CodexBar", 26.0, Weight::Semibold, pal.fg),
        horizontal_space(),
        container(t("LOCAL SNAPSHOT", 14.0, Weight::Semibold, pal.secondary()))
            .padding([9, 15])
            .style(move |_| container::Style {
                border: Border {
                    color: pal.tint(0.3),
                    width: 1.0,
                    radius: 12.0.into()
                },
                ..Default::default()
            }),
    ]
    .align_y(Alignment::Center)
    .height(34)
    .into()
}

fn hero<'a>(p: &SharePayload, pal: Palette) -> Element<'a, Message> {
    let (period, spend_label) = if p.is_all_time() {
        ("ALL".to_string(), "ALL-TIME SPEND".to_string())
    } else {
        (format!("{} DAYS", p.days), format!("{}-DAY SPEND", p.days))
    };
    let denominator = if p.is_all_time() {
        "all".to_string()
    } else {
        format!("{}d", p.days)
    };
    let left = column![
        t(
            format!("TRACKED TOKENS · {period}"),
            20.0,
            Weight::Semibold,
            pal.secondary()
        ),
        t(share::tokens_text(p), 104.0, Weight::Semibold, pal.fg),
    ]
    .spacing(2)
    .width(Length::Fill);

    let mut right = column![t(
        format!("EST. {spend_label}"),
        18.0,
        Weight::Semibold,
        pal.secondary()
    )]
    .spacing(9);
    for c in p.currencies.iter().take(2) {
        right = right.push(
            row![
                t(
                    format!("{} · {}/{denominator}", c.code, c.covered_days),
                    17.0,
                    Weight::Semibold,
                    pal.secondary()
                ),
                horizontal_space(),
                t(share::spend_text(c), 32.0, Weight::Semibold, pal.fg),
            ]
            .align_y(Alignment::End),
        );
    }
    let hidden = p.currencies.len().saturating_sub(2);
    let summary = if hidden > 0 {
        format!("+{hidden} more currencies · see subscription rows")
    } else {
        format!(
            "{} · native currencies kept separate",
            share::subscription_summary(p.providers.len())
        )
    };
    right = right.push(t(summary, 16.0, Weight::Medium, pal.secondary()));

    row![left, container(right).width(390)]
        .spacing(52)
        .align_y(Alignment::End)
        .height(132)
        .into()
}

fn section_header<'a>(title: &str, detail: String, pal: Palette) -> Element<'a, Message> {
    row![
        t(title.to_string(), 18.0, Weight::Semibold, pal.secondary()),
        horizontal_space(),
        t(detail, 16.0, Weight::Semibold, pal.secondary()),
    ]
    .align_y(Alignment::End)
    .into()
}

fn icon<'a>(id: &str) -> Element<'a, Message> {
    let (r, g, b) = crate::descriptors::accent_hex(id)
        .and_then(super::popover::parse_hex_rgb)
        .unwrap_or((0x88, 0x88, 0x88));
    container(t(
        crate::descriptors::tag(id),
        10.0,
        Weight::Bold,
        Color::WHITE,
    ))
    .width(24)
    .height(24)
    .align_x(Alignment::Center)
    .align_y(Alignment::Center)
    .style(move |_| container::Style {
        background: Some(Background::Color(Color::from_rgb8(r, g, b))),
        border: Border {
            radius: 12.0.into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .into()
}

fn rank_text<'a>(rank: Option<usize>, pal: Palette) -> Element<'a, Message> {
    container(t(
        rank.map_or("·".to_string(), |r| format!("{r:02}")),
        17.0,
        Weight::Bold,
        pal.secondary(),
    ))
    .width(27)
    .into()
}

fn row_frame<'a>(content: Element<'a, Message>, height: f32, pal: Palette) -> Element<'a, Message> {
    container(content)
        .padding([0, 9])
        .height(height)
        .width(Length::Fill)
        .align_y(Alignment::Center)
        .style(move |_| container::Style {
            background: Some(Background::Color(pal.tint(0.035))),
            border: Border {
                color: pal.tint(0.10),
                width: 1.0,
                radius: 9.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn provider_row<'a>(
    rank: usize,
    r: &share::ProviderRow,
    days: i64,
    pal: Palette,
) -> Element<'a, Message> {
    let mut name = row![t(r.name.clone(), 20.0, Weight::Semibold, pal.fg)]
        .spacing(8)
        .align_y(Alignment::End);
    if let Some(plan) = &r.plan {
        name = name.push(t(
            format!("· {plan}"),
            17.0,
            Weight::Medium,
            pal.secondary(),
        ));
    }
    row_frame(
        row![
            icon(&r.id),
            Space::with_width(9),
            rank_text(Some(rank), pal),
            Space::with_width(9),
            name,
            horizontal_space(),
            t(
                share::provider_detail(r, days),
                18.0,
                Weight::Medium,
                pal.secondary()
            ),
        ]
        .align_y(Alignment::Center)
        .into(),
        44.0,
        pal,
    )
}

fn model_row<'a>(rank: Option<usize>, m: &share::ModelRow, pal: Palette) -> Element<'a, Message> {
    row_frame(
        row![
            icon(&m.provider_id),
            Space::with_width(9),
            rank_text(rank, pal),
            Space::with_width(9),
            column![
                t(m.model.clone(), 20.0, Weight::Semibold, pal.fg),
                t(
                    m.provider_name.clone(),
                    16.0,
                    Weight::Medium,
                    pal.secondary()
                ),
            ]
            .spacing(1),
            horizontal_space(),
            t(
                share::model_detail(m),
                17.0,
                Weight::Medium,
                pal.secondary()
            ),
        ]
        .align_y(Alignment::Center)
        .into(),
        48.0,
        pal,
    )
}

fn rankings<'a>(p: &SharePayload, pal: Palette) -> Element<'a, Message> {
    let count = p.providers.len();
    let limit = if count > 5 { 4 } else { count.min(5) };
    let mut subs = column![section_header(
        "SUBSCRIPTIONS",
        format!("{count} CONNECTED"),
        pal
    )]
    .spacing(6);
    for (i, r) in p.providers.iter().take(limit).enumerate() {
        subs = subs.push(provider_row(i + 1, r, p.days, pal));
    }
    if count > limit {
        subs = subs.push(t(
            format!("+{} more configured", count - limit),
            16.0,
            Weight::Medium,
            pal.secondary(),
        ));
    }

    let detail = if p.partial_models {
        "PARTIAL"
    } else {
        "BY USAGE"
    };
    let mut models = column![section_header("TOP MODELS", detail.to_string(), pal)].spacing(6);
    if p.top_models.is_empty() {
        models = models.push(t(
            "No model-level history in this local snapshot",
            18.0,
            Weight::Medium,
            pal.secondary(),
        ));
    } else {
        for (i, m) in p.top_models.iter().take(3).enumerate() {
            models = models.push(model_row((!p.partial_models).then_some(i + 1), m, pal));
        }
    }
    let right = column![
        models,
        Space::with_height(18),
        t(
            "Only aggregate usage, plan tier, and estimated spend are included.",
            16.0,
            Weight::Medium,
            pal.secondary()
        ),
    ];
    row![
        container(subs).width(554),
        container(right).width(Length::Fill)
    ]
    .spacing(46)
    .height(286)
    .into()
}

fn footer<'a>(p: &SharePayload, pal: Palette) -> Element<'a, Message> {
    row![
        t(
            "LOCAL · AGGREGATE ONLY",
            14.0,
            Weight::Medium,
            pal.secondary()
        ),
        horizontal_space(),
        t(
            format!(
                "DATA THROUGH {}",
                share::data_through(&p.period_end).to_uppercase()
            ),
            14.0,
            Weight::Medium,
            pal.secondary()
        ),
    ]
    .into()
}

/// The 1200x630 card. `None` payload renders an explicit empty state.
pub fn card<'a>(payload: Option<&SharePayload>, dark: bool) -> Element<'a, Message> {
    let pal = Palette::new(dark);
    let body: Element<'a, Message> = match payload {
        Some(p) => column![
            header(pal),
            Space::with_height(16),
            hero(p, pal),
            Space::with_height(17),
            container(Space::new(Length::Fill, 1)).style(move |_| container::Style {
                background: Some(Background::Color(Color {
                    a: 0.22,
                    ..pal.secondary()
                })),
                ..Default::default()
            }),
            Space::with_height(17),
            rankings(p, pal),
            Space::with_height(Length::Fill),
            footer(p, pal),
        ]
        .into(),
        None => column![
            header(pal),
            Space::with_height(Length::Fill),
            t("Nothing to share yet", 40.0, Weight::Semibold, pal.fg),
            t(
                "No provider has measured usage or spend in this window. Refresh costs from Usage & Spend.",
                20.0,
                Weight::Medium,
                pal.secondary(),
            ),
            Space::with_height(Length::Fill),
        ]
        .into(),
    };
    container(body)
        .padding([34, 52])
        .width(CARD_W)
        .height(CARD_H)
        .clip(true)
        .style(move |_| container::Style {
            background: Some(Background::Color(pal.bg)),
            ..Default::default()
        })
        .into()
}

pub fn current_payload(app: &App) -> Option<SharePayload> {
    share::build(
        &app.spending,
        &app.spend,
        &app.entries,
        app.spend_period_days,
        &chrono::Local::now().format("%Y-%m-%d").to_string(),
    )
}

fn choice<'a>(label: &str, selected: bool, msg: Message) -> Element<'a, Message> {
    let b = crate::ui::style::button(text(label.to_string())).on_press(msg);
    if selected {
        b.style(crate::ui::style::segment(true))
    } else {
        b.style(crate::ui::style::segment(false))
    }
    .into()
}

pub fn view(app: &App) -> Element<'_, Message> {
    let payload = current_payload(app);
    let has = payload.is_some();
    let p = app.spend_period_days;
    let status = if app.share_status.is_empty() {
        "Nothing is uploaded. This image is created on your computer.".to_string()
    } else {
        app.share_status.clone()
    };
    let controls = column![
        row![
            text("Period").size(13),
            choice("7d", p == 7, Message::ShareSetPeriod(7)),
            choice("30d", p == 30, Message::ShareSetPeriod(30)),
            choice("90d", p == 90, Message::ShareSetPeriod(90)),
            choice("All", p <= 0, Message::ShareSetPeriod(0)),
            Space::with_width(16),
            text("Card").size(13),
            choice("Dark", app.share_dark, Message::ShareSetDark(true)),
            choice("Light", !app.share_dark, Message::ShareSetDark(false)),
            horizontal_space(),
            crate::ui::style::button(text("Copy image"))
                .on_press_maybe(has.then_some(Message::ShareCopyImage)),
            crate::ui::style::button(text("Copy stats"))
                .on_press_maybe(has.then_some(Message::ShareCopyText)),
            crate::ui::style::button(text("Save PNG…"))
                .on_press_maybe(has.then_some(Message::ShareSave)),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        text(status).size(13),
    ]
    .spacing(8);
    column![card(payload.as_ref(), app.share_dark), controls]
        .spacing(16)
        .padding(PAD)
        .into()
}
