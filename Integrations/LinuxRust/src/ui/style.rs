//! Shared macOS-flavoured look: theme palette, button / input / card / progress styles.
//!
//! Metrics mirror the SwiftUI app: 6pt-radius bordered buttons, accent fill only on the
//! selected segment/row, hairline separators, thin capsule meters and rounded subtle cards.

use iced::overlay::menu;
use iced::widget::{button as button_widget, text_input as text_input_widget};
use iced::widget::{
    button::{self, Button},
    checkbox::{self},
    container,
    pick_list::{self},
    progress_bar,
    radio::{self},
    scrollable::{self},
    slider::{self},
    text_input::{self, TextInput},
    toggler::{self},
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

/// 14pt rounded checkbox: hairline border when off, accent fill + white check when on.
pub fn checkbox<'a, M>(label: impl ToString, checked: bool) -> iced::widget::Checkbox<'a, M> {
    iced::widget::checkbox(label.to_string(), checked)
        .size(14)
        .spacing(8)
        .style(checkbox_style)
}

pub fn checkbox_style(theme: &Theme, status: checkbox::Status) -> checkbox::Style {
    let (checked, hovered, disabled) = match status {
        checkbox::Status::Active { is_checked } => (is_checked, false, false),
        checkbox::Status::Hovered { is_checked } => (is_checked, true, false),
        checkbox::Status::Disabled { is_checked } => (is_checked, false, true),
    };
    let fade = |c: Color| {
        if disabled {
            Color { a: c.a * 0.4, ..c }
        } else {
            c
        }
    };
    let (fill, border) = control_colors(checked, hovered);
    checkbox::Style {
        background: Background::Color(fade(fill)),
        icon_color: fade(Color::WHITE),
        border: Border {
            color: fade(border),
            width: 1.0,
            radius: 4.0.into(),
        },
        text_color: Some(fade(theme.palette().text)),
    }
}

/// Fill and hairline colours shared by checkbox and radio (on = accent, off = faint with ring).
fn control_colors(on: bool, hovered: bool) -> (Color, Color) {
    match (on, hovered) {
        (true, true) => (Color::from_rgb(0.2, 0.58, 1.0), Color::TRANSPARENT),
        (true, false) => (ACCENT, Color::TRANSPARENT),
        (false, true) => (white(0.10), white(0.30)),
        (false, false) => (white(0.05), white(0.22)),
    }
}

/// 14pt radio: hairline ring, accent fill + white dot when selected.
pub fn radio<'a, M: Clone, V: Copy + Eq>(
    label: impl Into<String>,
    value: V,
    selected: Option<V>,
    on_click: impl FnOnce(V) -> M,
) -> iced::widget::Radio<'a, M> {
    iced::widget::radio(label, value, selected, on_click)
        .size(14)
        .spacing(8)
        .style(radio_style)
}

pub fn radio_style(theme: &Theme, status: radio::Status) -> radio::Style {
    let (selected, hovered) = match status {
        radio::Status::Active { is_selected } => (is_selected, false),
        radio::Status::Hovered { is_selected } => (is_selected, true),
    };
    let (fill, ring) = control_colors(selected, hovered);
    radio::Style {
        background: Background::Color(fill),
        dot_color: Color::WHITE,
        border_width: 1.0,
        border_color: ring,
        text_color: Some(theme.palette().text),
    }
}

/// Thin rail (accent left of the handle, translucent grey right) with a round white handle.
pub fn slider(_theme: &Theme, status: slider::Status) -> slider::Style {
    let grow = matches!(status, slider::Status::Hovered | slider::Status::Dragged);
    slider::Style {
        rail: slider::Rail {
            backgrounds: (Background::Color(ACCENT), Background::Color(white(0.18))),
            width: 4.0,
            border: Border::default().rounded(2.0),
        },
        handle: slider::Handle {
            shape: slider::HandleShape::Circle {
                radius: if grow { 8.0 } else { 7.0 },
            },
            background: Background::Color(Color::WHITE),
            border_width: 0.5,
            border_color: Color::from_rgba(0.0, 0.0, 0.0, 0.35),
        },
    }
}

/// Mac-style switch: capsule track (accent when on) with a round white knob.
#[allow(dead_code)]
pub fn toggler(_theme: &Theme, status: toggler::Status) -> toggler::Style {
    let (on, hovered) = match status {
        toggler::Status::Active { is_toggled } => (is_toggled, false),
        toggler::Status::Hovered { is_toggled } => (is_toggled, true),
        toggler::Status::Disabled => (false, false),
    };
    let track = match (on, hovered) {
        (true, true) => Color::from_rgb(0.2, 0.58, 1.0),
        (true, false) => ACCENT,
        (false, true) => white(0.26),
        (false, false) => white(0.18),
    };
    toggler::Style {
        background: track,
        background_border_width: 0.0,
        background_border_color: Color::TRANSPARENT,
        foreground: Color::WHITE,
        foreground_border_width: 0.5,
        foreground_border_color: Color::from_rgba(0.0, 0.0, 0.0, 0.3),
    }
}

/// Bordered, rounded drop-down matching the push buttons.
pub fn pick_list_style(theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let fill = match status {
        pick_list::Status::Active => white(0.09),
        pick_list::Status::Hovered => white(0.14),
        pick_list::Status::Opened => white(0.20),
    };
    pick_list::Style {
        text_color: theme.palette().text,
        placeholder_color: white(0.35),
        handle_color: SECONDARY,
        background: Background::Color(fill),
        border: Border {
            color: if matches!(status, pick_list::Status::Opened) {
                ACCENT
            } else {
                white(0.10)
            },
            width: 1.0,
            radius: 6.0.into(),
        },
    }
}

/// Dark rounded popup menu with an accent highlight on the hovered row.
pub fn pick_menu(theme: &Theme) -> menu::Style {
    menu::Style {
        background: Background::Color(Color::from_rgb(0.18, 0.18, 0.19)),
        border: Border {
            color: white(0.14),
            width: 1.0,
            radius: 8.0.into(),
        },
        text_color: theme.palette().text,
        selected_text_color: Color::WHITE,
        selected_background: Background::Color(ACCENT),
    }
}

/// Drop-down with the mac look (field and popup menu).
pub fn pick_list<'a, T, L, V, M>(
    options: L,
    selected: Option<V>,
    on_select: impl Fn(T) -> M + 'a,
) -> iced::widget::PickList<'a, T, L, V, M>
where
    T: ToString + PartialEq + Clone + 'a,
    L: std::borrow::Borrow<[T]> + 'a,
    V: std::borrow::Borrow<T> + 'a,
    M: Clone,
{
    iced::widget::pick_list(options, selected, on_select)
        .padding([4, 8])
        .style(pick_list_style)
        .menu_style(pick_menu)
}

/// Overlay-style scroller: narrow translucent rounded thumb, no visible track.
pub fn scrollbar() -> scrollable::Scrollbar {
    scrollable::Scrollbar::new()
        .width(4)
        .scroller_width(4)
        .margin(2)
}

pub fn scroll_style(_theme: &Theme, status: scrollable::Status) -> scrollable::Style {
    let alpha = match status {
        scrollable::Status::Active => 0.30,
        scrollable::Status::Hovered { .. } => 0.45,
        scrollable::Status::Dragged { .. } => 0.60,
    };
    let rail = scrollable::Rail {
        background: None,
        border: Border::default(),
        scroller: scrollable::Scroller {
            color: white(alpha),
            border: Border::default().rounded(2.0),
        },
    };
    scrollable::Style {
        container: container::Style::default(),
        vertical_rail: rail,
        horizontal_rail: rail,
        gap: None,
    }
}

/// Vertically scrolling container with the thin overlay scroller.
pub fn scrollable<'a, M>(content: impl Into<Element<'a, M>>) -> iced::widget::Scrollable<'a, M> {
    iced::widget::scrollable(content)
        .direction(scrollable::Direction::Vertical(scrollbar()))
        .style(scroll_style)
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
