//! Menu Bar layout editor (port of `MenuBarLayoutEditor` /
//! `MenuBarLayoutConditionalEditor`): token chips per line with reorder and
//! remove, a token palette, the conditional library, and a live preview built
//! with the very same `tray::tray_view` the tray uses.
//! Every control emits `Message::LayoutEdit`, applied by `menubar_layout::apply`.

use crate::menubar_layout::{
    Combinator, Comparison, Conditional, Direction, DisplayMode, LayoutEdit, MenuBarLayout, Metric,
    MetricKind, Piece, Preset, RenderInput, TitleMode, Token, MAX_CLAUSES, MAX_LINES,
};
use crate::state::{App, Message};
use crate::tray::{self, TrayIcon, TrayParams};
use crate::ui::style::pick_list;
use iced::widget::{column, container, image, row, text};
use iced::{Color, Element, Length};
use std::fmt;

/// pick_list item: compared by value, shown by label.
#[derive(Clone, PartialEq)]
struct Choice<T>(T, String);

impl<T> fmt::Display for Choice<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.1)
    }
}

fn dim(s: String) -> Element<'static, Message> {
    text(s)
        .size(12)
        .color(Color::from_rgb(0.65, 0.65, 0.65))
        .into()
}

fn edit(e: LayoutEdit) -> Message {
    Message::LayoutEdit(e)
}

fn small_button(label: &'static str, msg: Option<Message>) -> Element<'static, Message> {
    crate::ui::style::button(text(label).size(12))
        .padding([1, 6])
        .style(crate::ui::style::push)
        .on_press_maybe(msg)
        .into()
}

pub fn view(app: &App) -> Element<'_, Message> {
    let layout = &app.menubar_layout;
    let mut col = column![
        text("Menu bar layout").size(16),
        preview(app),
        presets(),
        title_modes(layout),
    ]
    .spacing(10);
    for line in 0..MAX_LINES {
        col = col.push(line_editor(layout, line));
    }
    col = col.push(conditionals(layout));
    col.into()
}

fn preview(app: &App) -> Element<'_, Message> {
    let layout = &app.menubar_layout;
    let entry = app.entries.first();
    let cost = entry.and_then(|e| app.spending.iter().find(|c| c.provider == e.provider));
    let s = &app.settings;
    let quota_used = s.quota_display == "used";
    let now = chrono::Utc::now();
    let view = tray::tray_view(
        layout,
        entry,
        cost,
        &TrayParams {
            tray_style: &s.tray_style,
            stale: app.stale,
            quota_used,
            warning_colors: s.warning_colors,
            notify_threshold: s.notify_threshold,
        },
        now,
    );
    let rendered = crate::menubar_layout::render(
        layout,
        &RenderInput {
            entry,
            cost,
            quota_used,
            now,
        },
    );
    let icon: Element<Message> = match view.icon {
        TrayIcon::Pixmap(rgba) => image(image::Handle::from_rgba(
            tray::PIXMAP_SIZE,
            tray::PIXMAP_SIZE,
            rgba,
        ))
        .width(Length::Fixed(32.0))
        .height(Length::Fixed(32.0))
        .into(),
        TrayIcon::Named => text("[themed icon]").size(12).into(),
    };
    let mut lines = column![].spacing(2);
    for line in &rendered.lines {
        let t: Vec<String> = line
            .iter()
            .map(|p| match p {
                Piece::Text(t) => t.clone(),
                Piece::Icon => "[icon]".into(),
                Piece::Bar(Some(v)) => format!("[bar {v}%]"),
                Piece::Bar(None) => "[bar \u{2014}]".into(),
                Piece::Space => " ".into(),
            })
            .collect();
        lines = lines.push(text(t.join(" ")).size(13));
    }
    let tray_box = container(
        row![icon, lines]
            .spacing(10)
            .align_y(iced::Alignment::Center),
    )
    .padding(10)
    .style(container::rounded_box);
    column![
        row![
            text("Preview").width(Length::Fixed(220.0)),
            tray_box
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
        dim(format!(
            "Tray title: {}. KDE shows the title next to the icon; most other panels show only the icon.",
            view.title
        )),
    ]
    .spacing(4)
    .into()
}

fn presets() -> Element<'static, Message> {
    let buttons: Vec<Element<'static, Message>> = Preset::ALL
        .iter()
        .map(|p| {
            crate::ui::style::button(text(p.label()).size(12))
                .padding([3, 8])
                .style(crate::ui::style::push)
                .on_press(edit(LayoutEdit::Preset(*p)))
                .into()
        })
        .collect();
    column![
        text("Presets"),
        iced::widget::Row::from_vec(buttons).spacing(6).wrap()
    ]
    .spacing(6)
    .into()
}

fn title_modes(layout: &MenuBarLayout) -> Element<'_, Message> {
    let title = pick_list(
        vec![
            Choice(TitleMode::Static, "Static (CodexBar)".to_string()),
            Choice(TitleMode::Layout, "From layout text".to_string()),
        ],
        Some(Choice(
            layout.title_mode,
            match layout.title_mode {
                TitleMode::Static => "Static (CodexBar)",
                TitleMode::Layout => "From layout text",
            }
            .to_string(),
        )),
        |c: Choice<TitleMode>| edit(LayoutEdit::SetTitleMode(c.0)),
    );
    let modes: Vec<_> = DisplayMode::ALL
        .iter()
        .map(|m| Choice(*m, m.label().to_string()))
        .collect();
    let mode = pick_list(
        modes,
        Some(Choice(
            layout.display_mode,
            layout.display_mode.label().to_string(),
        )),
        |c: Choice<DisplayMode>| edit(LayoutEdit::SetDisplayMode(c.0)),
    );
    column![
        row![text("Tray title").width(Length::Fixed(220.0)), title]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        row![text("Display mode text").width(Length::Fixed(220.0)), mode]
            .spacing(8)
            .align_y(iced::Alignment::Center),
    ]
    .spacing(6)
    .into()
}

fn token_choices(layout: &MenuBarLayout, allow_conditional: bool) -> Vec<Choice<Token>> {
    let mut tokens = layout.palette();
    tokens.push(Token::Hidden);
    tokens
        .into_iter()
        .filter(|t| allow_conditional || !matches!(t, Token::Conditional { .. }))
        .map(|t| {
            let label = layout.token_label(&t);
            Choice(t, label)
        })
        .collect()
}

fn line_editor(layout: &MenuBarLayout, line: usize) -> Element<'_, Message> {
    let tokens: &[Token] = layout.lines.get(line).map(Vec::as_slice).unwrap_or(&[]);
    let mut chips = row![].spacing(6);
    for (index, token) in tokens.iter().enumerate() {
        let move_msg = |delta: i32| {
            let target = index as i64 + delta as i64;
            (target >= 0 && (target as usize) < tokens.len())
                .then(|| edit(LayoutEdit::MoveToken { line, index, delta }))
        };
        chips = chips.push(
            container(
                row![
                    small_button("<", move_msg(-1)),
                    text(layout.token_label(token)).size(13),
                    small_button(">", move_msg(1)),
                    small_button("x", Some(edit(LayoutEdit::RemoveToken { line, index }))),
                ]
                .spacing(4)
                .align_y(iced::Alignment::Center),
            )
            .padding([3, 6])
            .style(container::rounded_box),
        );
    }
    if tokens.is_empty() {
        chips = chips.push(dim("(empty)".into()));
    }
    let add = pick_list(
        token_choices(layout, true),
        None::<Choice<Token>>,
        move |c: Choice<Token>| edit(LayoutEdit::AddToken { line, token: c.0 }),
    )
    .placeholder("+ Add token");
    column![
        row![
            text(if line == 0 {
                "Line 1"
            } else {
                "Line 2 (stacked)"
            })
            .width(Length::Fixed(220.0)),
            add
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
        chips.wrap(),
    ]
    .spacing(6)
    .into()
}

fn conditionals(layout: &MenuBarLayout) -> Element<'_, Message> {
    let mut col = column![
        row![
            text("Conditionals").size(16),
            small_button("+ New conditional", Some(edit(LayoutEdit::AddConditional))),
        ]
        .spacing(10)
        .align_y(iced::Alignment::Center),
        dim("A conditional shows one token when its rules match and another otherwise. Add it to a line from the token list. Unknown data never matches.".into()),
    ]
    .spacing(8);
    for conditional in &layout.conditionals {
        col = col.push(conditional_editor(layout, conditional));
    }
    col.into()
}

fn conditional_editor<'a>(layout: &'a MenuBarLayout, c: &'a Conditional) -> Element<'a, Message> {
    let id = c.id.clone();
    let name_id = id.clone();
    let mut col = column![row![
        text(format!("{}:", c.id)).size(13),
        crate::ui::style::text_input("Name", &c.name)
            .on_input(move |v| edit(LayoutEdit::SetConditionalName(name_id.clone(), v)))
            .width(Length::Fixed(220.0)),
        small_button(
            "Remove",
            Some(edit(LayoutEdit::RemoveConditional(id.clone())))
        ),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center)]
    .spacing(6);

    for (i, clause) in c.clauses.iter().enumerate() {
        let p = &clause.predicate;
        let mut r = row![].spacing(6).align_y(iced::Alignment::Center);
        if i == 0 {
            r = r.push(text("If").width(Length::Fixed(34.0)).size(13));
        } else {
            let id2 = id.clone();
            let cur = clause.combinator.unwrap_or(Combinator::And);
            r = r.push(
                pick_list(
                    vec![
                        Choice(Combinator::And, "and".to_string()),
                        Choice(Combinator::Or, "or".to_string()),
                    ],
                    Some(Choice(
                        cur,
                        if cur == Combinator::And { "and" } else { "or" }.to_string(),
                    )),
                    move |ch: Choice<Combinator>| {
                        edit(LayoutEdit::SetCombinator(id2.clone(), i, ch.0))
                    },
                )
                .width(Length::Fixed(60.0)),
            );
        }
        let id_m = id.clone();
        r = r.push(
            pick_list(
                Metric::ALL
                    .iter()
                    .map(|m| Choice(*m, m.label().to_string()))
                    .collect::<Vec<_>>(),
                Some(Choice(p.metric, p.metric.label().to_string())),
                move |ch: Choice<Metric>| edit(LayoutEdit::SetMetric(id_m.clone(), i, ch.0)),
            )
            .width(Length::Fixed(170.0)),
        );
        if p.metric.supports_direction() {
            let id_d = id.clone();
            let label = |d: Direction| match d {
                Direction::Used => "used",
                Direction::Remaining => "remaining",
            };
            r = r.push(
                pick_list(
                    vec![
                        Choice(Direction::Used, label(Direction::Used).to_string()),
                        Choice(
                            Direction::Remaining,
                            label(Direction::Remaining).to_string(),
                        ),
                    ],
                    Some(Choice(p.direction, label(p.direction).to_string())),
                    move |ch: Choice<Direction>| {
                        edit(LayoutEdit::SetDirection(id_d.clone(), i, ch.0))
                    },
                )
                .width(Length::Fixed(110.0)),
            );
        }
        let id_c = id.clone();
        r = r.push(
            pick_list(
                Comparison::ALL
                    .iter()
                    .map(|m| Choice(*m, m.symbol().to_string()))
                    .collect::<Vec<_>>(),
                Some(Choice(p.comparison, p.comparison.symbol().to_string())),
                move |ch: Choice<Comparison>| {
                    edit(LayoutEdit::SetComparison(id_c.clone(), i, ch.0))
                },
            )
            .width(Length::Fixed(64.0)),
        );
        let (lo, hi) = p.metric.range();
        r = r
            .push(small_button(
                "-",
                (p.threshold > lo).then(|| edit(LayoutEdit::StepThreshold(id.clone(), i, -1))),
            ))
            .push(
                text(format_threshold(p.metric, p.threshold))
                    .size(13)
                    .width(Length::Fixed(64.0)),
            )
            .push(small_button(
                "+",
                (p.threshold < hi).then(|| edit(LayoutEdit::StepThreshold(id.clone(), i, 1))),
            ));
        if c.clauses.len() > 1 {
            r = r.push(small_button(
                "x",
                Some(edit(LayoutEdit::RemoveClause(id.clone(), i))),
            ));
        }
        col = col.push(r);
    }
    if c.clauses.len() < MAX_CLAUSES {
        col = col.push(small_button(
            "+ Add rule",
            Some(edit(LayoutEdit::AddClause(id.clone()))),
        ));
    }
    for (then, label, token) in [
        (true, "Then", &c.then_token),
        (false, "Else", &c.else_token),
    ] {
        let id_b = id.clone();
        col = col.push(
            row![
                text(label).width(Length::Fixed(34.0)).size(13),
                pick_list(
                    token_choices(layout, false),
                    Some(Choice(token.clone(), layout.token_label(token))),
                    move |ch: Choice<Token>| edit(LayoutEdit::SetBranch {
                        id: id_b.clone(),
                        then,
                        token: ch.0,
                    }),
                ),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
        );
    }
    container(col)
        .padding(10)
        .width(Length::Fill)
        .style(container::rounded_box)
        .into()
}

fn format_threshold(metric: Metric, value: f64) -> String {
    match metric.kind() {
        MetricKind::Hours => format!("{value:.1} {}", metric.unit()),
        MetricKind::Usd => format!("{value:.0} {}", metric.unit()),
        _ => format!("{value:.0}{}", metric.unit()),
    }
}
