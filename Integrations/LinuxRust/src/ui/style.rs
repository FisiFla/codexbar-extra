//! Shared macOS-flavoured look: theme palette, button / input / card / progress styles.
//!
//! Metrics mirror the SwiftUI app: 6pt-radius bordered buttons, accent fill only on the
//! selected segment/row, hairline separators, thin capsule meters and rounded subtle cards.

use iced::widget::{button as button_widget, text_input as text_input_widget};
use iced::widget::{
    button::{self, Button},
    container, progress_bar,
    text_input::{self, TextInput},
};
use iced::{Background, Border, Color, Element, Theme};

/// macOS system accent blue (dark appearance).
pub const ACCENT: Color = Color::from_rgb(0.039, 0.518, 1.0);

/// Secondary label colour (macOS `secondaryLabelColor`, dark).
pub const SECONDARY: Color = Color::from_rgb(0.62, 0.62, 0.65);

/// Dark theme tuned toward macOS: neutral graphite surfaces, system accent colours.
pub fn mac_theme() -> Theme {
    Theme::custom(
        "CodexBar".to_string(),
        iced::theme::Palette {
            background: Color::from_rgb(0.141, 0.141, 0.149),
            text: Color::from_rgb(0.95, 0.95, 0.97),
            primary: ACCENT,
            success: Color::from_rgb(0.188, 0.82, 0.345),
            danger: Color::from_rgb(1.0, 0.27, 0.227),
        },
    )
}

fn white(a: f32) -> Color {
    Color::from_rgba(1.0, 1.0, 1.0, a)
}

/// Bordered push button with the shared look (override with `.style(..)` as needed).
pub fn button<'a, M: Clone + 'a>(content: impl Into<Element<'a, M>>) -> Button<'a, M> {
    button_widget(content).style(push).padding([4, 12])
}

/// Rounded, hairline-bordered text field with an accent focus ring.
pub fn text_input<'a, M: Clone + 'a>(placeholder: &str, value: &str) -> TextInput<'a, M> {
    text_input_widget(placeholder, value)
        .style(input)
        .padding([5, 8])
}

pub fn input(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let p = theme.palette();
    let border = match status {
        text_input::Status::Focused => (ACCENT, 1.5),
        text_input::Status::Hovered => (white(0.22), 1.0),
        _ => (white(0.12), 1.0),
    };
    text_input::Style {
        background: Background::Color(white(0.05)),
        border: Border {
            color: border.0,
            width: border.1,
            radius: 6.0.into(),
        },
        icon: SECONDARY,
        placeholder: Color::from_rgba(1.0, 1.0, 1.0, 0.35),
        value: p.text,
        selection: Color { a: 0.45, ..ACCENT },
    }
}

/// Default bordered push button.
pub fn push(theme: &Theme, status: button::Status) -> button::Style {
    let text = theme.palette().text;
    let (fill, text_color) = match status {
        button::Status::Hovered => (white(0.14), text),
        button::Status::Pressed => (white(0.20), text),
        button::Status::Disabled => (white(0.04), Color { a: 0.4, ..text }),
        button::Status::Active => (white(0.09), text),
    };
    button::Style {
        background: Some(Background::Color(fill)),
        text_color,
        border: Border {
            color: white(0.10),
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    }
}

/// Segmented-picker segment: accent only on the selected one, flat subtle fill otherwise.
pub fn segment(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        if selected {
            let fill = match status {
                button::Status::Hovered => Color::from_rgb(0.2, 0.58, 1.0),
                button::Status::Pressed => Color::from_rgb(0.0, 0.45, 0.9),
                _ => ACCENT,
            };
            button::Style {
                background: Some(Background::Color(fill)),
                text_color: Color::WHITE,
                border: Border::default().rounded(6.0),
                ..Default::default()
            }
        } else {
            let mut s = push(theme, status);
            s.border.color = Color::TRANSPARENT;
            s
        }
    }
}

/// Source-list row: plain text, rounded accent highlight when selected.
pub fn sidebar(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let text = theme.palette().text;
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let (background, text_color) = if selected {
            (Some(Background::Color(ACCENT)), Color::WHITE)
        } else if hovered {
            (Some(Background::Color(white(0.07))), text)
        } else {
            (None, text)
        };
        button::Style {
            background,
            text_color,
            border: Border::default().rounded(6.0),
            ..Default::default()
        }
    }
}

/// Rounded card with a subtle fill and hairline border.
pub fn card(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(white(0.045))),
        border: Border {
            color: white(0.08),
            width: 1.0,
            radius: 10.0.into(),
        },
        ..Default::default()
    }
}

/// Thin capsule meter: translucent track, solid `fill`.
pub fn meter(fill: Color, height: f32) -> impl Fn(&Theme) -> progress_bar::Style {
    move |_| progress_bar::Style {
        background: Background::Color(white(0.14)),
        bar: Background::Color(fill),
        border: Border::default().rounded(height / 2.0),
    }
}

/// Clickable card (overview rows): the card look with a brighter fill on hover.
pub fn card_button(_theme: &Theme, hovered: bool) -> button::Style {
    button::Style {
        background: Some(Background::Color(white(if hovered { 0.09 } else { 0.05 }))),
        text_color: Color::WHITE,
        border: Border {
            color: white(0.08),
            width: 1.0,
            radius: 10.0.into(),
        },
        ..Default::default()
    }
}
