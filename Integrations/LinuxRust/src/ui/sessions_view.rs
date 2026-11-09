//! Agent Sessions window (mac Agent Sessions menu section): live sessions from
//! `codexbar sessions --json-v2`, grouped by state (active first) then provider.

use crate::descriptors;
use crate::sessions::{self, Session};
use crate::state::{App, Message};
use crate::ui::popover::accent_rgb;
use crate::ui::style::scrollable;
use iced::widget::{column, container, horizontal_space, row, text, Column, Space};
use iced::{Background, Border, Color, Element, Font, Length, Theme};

fn muted() -> Color {
    Color::from_rgba(1.0, 1.0, 1.0, 0.65)
}

fn semibold() -> Font {
    Font {
        weight: iced::font::Weight::Semibold,
        ..Font::DEFAULT
    }
}

fn dot(color: Color) -> Element<'static, Message> {
    container(Space::new(8, 8))
        .style(move |_| container::Style {
            background: Some(Background::Color(color)),
            border: Border::default().rounded(4.0),
            ..Default::default()
        })
        .into()
}

fn small_button(label: &'static str, msg: Option<Message>) -> Element<'static, Message> {
    crate::ui::style::button(text(label).size(12))
        .on_press_maybe(msg)
        .padding([4, 10])
        .style(crate::ui::style::push)
        .into()
}

fn session_card(s: &Session, now: chrono::DateTime<chrono::Utc>) -> Element<'static, Message> {
    let (r, g, b) = accent_rgb(&s.provider);
    let accent = Color::from_rgb8(r, g, b);
    let cwd = s.cwd.clone().filter(|c| !c.is_empty());
    let mut meta: Vec<String> = Vec::new();
    if let Some(pid) = s.pid {
        meta.push(format!("pid {pid}"));
    }
    meta.push(s.source_label().to_string());
    if !s.host.is_empty() {
        meta.push(s.host.clone());
    }
    let mut body = column![row![
        dot(accent),
        text(s.title()).size(14).font(semibold()),
        horizontal_space(),
        text(sessions::relative_time(s.last_activity(), now))
            .size(12)
            .color(muted()),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center)]
    .spacing(3);
    if let Some(c) = &cwd {
        body = body.push(text(c.clone()).size(12).color(muted()));
    }
    body = body.push(text(meta.join(" \u{00b7} ")).size(12).color(muted()));
    let open = cwd.clone().map(Message::OpenFolder);
    let copy = s
        .transcript_path
        .clone()
        .filter(|p| !p.is_empty())
        .map(Message::CopyText);
    let focus = s.pid.map(Message::FocusSession);
    body = body.push(
        row![
            small_button("Focus", focus),
            small_button("Open folder", open),
            small_button("Copy transcript path", copy),
        ]
        .spacing(6),
    );
    container(body.padding(10))
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.05))),
            border: Border::default().rounded(8.0),
            ..Default::default()
        })
        .into()
}

fn groups_view(app: &App) -> Element<'static, Message> {
    let now = chrono::Utc::now();
    let mut col = Column::new().spacing(8);
    for sg in sessions::group(&app.sessions) {
        let total: usize = sg.providers.iter().map(|p| p.sessions.len()).sum();
        col = col.push(
            text(format!("{} ({total})", sg.state.label()))
                .size(15)
                .font(semibold()),
        );
        for pg in &sg.providers {
            let (r, g, b) = accent_rgb(&pg.provider);
            col = col.push(
                row![
                    dot(Color::from_rgb8(r, g, b)),
                    text(format!(
                        "{} \u{00b7} {}",
                        descriptors::display_name(&pg.provider),
                        pg.sessions.len()
                    ))
                    .size(12)
                    .color(muted()),
                ]
                .spacing(6)
                .align_y(iced::Alignment::Center),
            );
            for s in &pg.sessions {
                col = col.push(session_card(s, now));
            }
        }
        col = col.push(Space::with_height(6));
    }
    col.into()
}

pub fn view(app: &App) -> Element<Message> {
    let status = if app.sessions_busy {
        "Refreshing\u{2026}".to_string()
    } else {
        match app.sessions.len() {
            1 => "1 session".to_string(),
            n => format!("{n} sessions"),
        }
    };
    let header = row![
        text("Agent Sessions").size(20).font(semibold()),
        text(status).size(12).color(muted()),
        horizontal_space(),
        small_button(
            "Refresh",
            (!app.sessions_busy).then_some(Message::RefreshSessions)
        ),
    ]
    .spacing(12)
    .align_y(iced::Alignment::Center);
    let header = column![header]
        .push_maybe(
            (!app.focus_status.is_empty())
                .then(|| text(app.focus_status.clone()).size(12).color(muted())),
        )
        .spacing(4);

    let mut body = Column::new().spacing(10);
    if !app.sessions_error.is_empty() {
        let err = format!("Could not load sessions: {}", app.sessions_error);
        body = body.push(
            container(text(err).size(13).color(Color::from_rgb8(0xcf, 0x74, 0x43)))
                .padding(10)
                .width(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(Color::from_rgba(0.81, 0.45, 0.26, 0.12))),
                    border: Border::default().rounded(8.0),
                    ..Default::default()
                }),
        );
    }
    if app.sessions.is_empty() {
        if app.sessions_error.is_empty() {
            let msg = if app.sessions_loaded {
                "No agent sessions running."
            } else {
                "Loading sessions\u{2026}"
            };
            body = body.push(
                column![
                    text(msg).size(15),
                    text("Codex, Claude and Pi sessions appear here while they run.")
                        .size(12)
                        .color(muted()),
                ]
                .spacing(4),
            );
        }
    } else {
        body = body.push(groups_view(app));
    }
    container(
        column![
            header,
            scrollable(body.padding([0, 4])).height(Length::Fill)
        ]
        .spacing(14)
        .padding(20),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(theme.palette().background)),
        ..Default::default()
    })
    .into()
}
