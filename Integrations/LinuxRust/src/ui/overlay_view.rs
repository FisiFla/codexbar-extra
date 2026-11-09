//! Quota warning card and confetti canvas, drawn into undecorated transparent always-on-top
//! windows. Both dismiss on click; `main.rs` also expires them on a timer. If the compositor
//! ignores transparency or stacking, the worst case is a plain rectangle for a few seconds.

use crate::overlay::{self, Alert};
use crate::state::{App, Message};
use crate::ui::popover::accent_rgb;
use iced::widget::canvas::{self, Frame, Geometry};
use iced::widget::{column, container, mouse_area, text};
use iced::{
    mouse, Background, Border, Color, Element, Length, Point, Rectangle, Renderer, Size, Theme,
};
use std::time::Instant;

pub const ALERT_SIZE: (f32, f32) = (480.0, 150.0);
pub const CONFETTI_SIZE: (f32, f32) = (1000.0, 720.0);

/// Fully transparent theme for the overlay windows (the daemon clears each window to the
/// theme background, so this is what makes the area around the card see-through).
pub fn clear_theme() -> Theme {
    let mut p = Theme::Dark.palette();
    p.background = Color::TRANSPARENT;
    Theme::custom("overlay".to_string(), p)
}

pub fn alert_view(app: &App) -> Element<'_, Message> {
    let Some(state) = &app.alert else {
        return container(text("")).into();
    };
    let Alert { title, message } = &state.alert;
    let card = container(
        column![
            row_title(title),
            text(message.clone())
                .size(14)
                .color(Color::from_rgba(1.0, 1.0, 1.0, 0.75))
        ]
        .spacing(6),
    )
    .padding([16, 20])
    .width(Length::Fill)
    .style(|_| container::Style {
        background: Some(Background::Color(Color::from_rgba(0.11, 0.11, 0.13, 0.94))),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.10),
            width: 1.0,
            radius: 16.0.into(),
        },
        ..Default::default()
    });
    mouse_area(
        container(card)
            .padding(8)
            .center_y(Length::Fill)
            .width(Length::Fill),
    )
    .on_press(Message::DismissOverlays)
    .into()
}

fn row_title(title: &str) -> Element<'static, Message> {
    iced::widget::row![
        text("!").size(20).color(Color::from_rgb8(0xff, 0x9f, 0x0a)),
        text(title.to_string()).size(17).font(iced::Font {
            weight: iced::font::Weight::Semibold,
            ..iced::Font::DEFAULT
        }),
    ]
    .spacing(10)
    .align_y(iced::Alignment::Center)
    .into()
}

pub fn confetti_view(app: &App) -> Element<'_, Message> {
    let Some(state) = &app.confetti else {
        return container(text("")).into();
    };
    let (r, g, b) = accent_rgb(&state.provider);
    let accent = Color::from_rgb8(r, g, b);
    let lift = |c: f32, k: f32| c + (1.0 - c) * k;
    let palette = vec![
        accent,
        Color::from_rgb(
            lift(accent.r, 0.4),
            lift(accent.g, 0.4),
            lift(accent.b, 0.4),
        ),
        Color::from_rgb(accent.r * 0.7, accent.g * 0.7, accent.b * 0.7),
        Color::WHITE,
        Color::from_rgb8(0xff, 0xd6, 0x0a),
    ];
    let t = if overlay::forced() {
        1.1 // frozen so dev captures are reproducible
    } else {
        state.started.elapsed().as_secs_f32()
    };
    mouse_area(
        canvas::Canvas::new(Confetti { palette, t })
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .on_press(Message::DismissOverlays)
    .into()
}

struct Confetti {
    palette: Vec<Color>,
    t: f32,
}

impl canvas::Program<Message> for Confetti {
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
        for p in overlay::particles(bounds.width, bounds.height, self.palette.len()) {
            let (x, y, rot, alpha) = overlay::pose(&p, self.t);
            if alpha <= 0.0 {
                continue;
            }
            let mut c = self.palette[p.color % self.palette.len()];
            c.a = alpha;
            frame.with_save(|f| {
                f.translate(iced::Vector::new(x, y));
                f.rotate(rot);
                f.fill_rectangle(Point::new(-p.w / 2.0, -p.h / 2.0), Size::new(p.w, p.h), c);
            });
        }
        vec![frame.into_geometry()]
    }
}

pub struct AlertState {
    pub alert: Alert,
    pub started: Instant,
}

pub struct ConfettiState {
    pub provider: String,
    pub started: Instant,
}
