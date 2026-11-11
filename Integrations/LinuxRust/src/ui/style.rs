//! Shared macOS-flavoured look: theme palette, button / input / card / progress styles.
//!
//! Metrics mirror the SwiftUI app: 6pt-radius bordered buttons, accent fill only on the
//! selected segment/row, hairline separators, thin capsule meters and rounded subtle cards.

use iced::overlay::menu;
use iced::widget::{button as button_widget, text_input as text_input_widget};
use iced::widget::{
    button::{self, Button},
    container,
    pick_list::{self},
    progress_bar,
    scrollable::{self},
    slider::{self},
    text_input::{self, TextInput},
    toggler::{self},
};
use iced::{Background, Border, Color, Element, Font, Theme};

/// macOS system accent blue (dark appearance).
pub const ACCENT: Color = Color::from_rgb(0.039, 0.518, 1.0);

/// Secondary label colour (macOS `secondaryLabelColor`, dark).
pub const SECONDARY: Color = Color::from_rgb(0.62, 0.62, 0.65);

/// Tertiary label colour (macOS `tertiaryLabelColor`, dark).
#[allow(dead_code)]
pub const TERTIARY: Color = Color::from_rgb(0.44, 0.44, 0.47);

/// Hairline separator (macOS `separatorColor` on dark).
pub const SEPARATOR: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.10);

/// System red (dark) for errors.
pub const ERROR: Color = Color::from_rgb(1.0, 0.42, 0.38);

/// System orange (dark) for warnings.
#[allow(dead_code)]
pub const WARNING: Color = Color::from_rgb(1.0, 0.62, 0.04);

// Type scale (macOS semantic ladder, px). Exact SwiftUI pts are OS-resolved;
// these follow the documented macOS defaults: body/headline 13, callout 12,
// caption 11, caption2 10, title3 15, title2 17, title 22.
#[allow(dead_code)]
pub const TEXT_TITLE: u16 = 22;
pub const TEXT_TITLE2: u16 = 17;
#[allow(dead_code)]
pub const TEXT_TITLE3: u16 = 15;
pub const TEXT_HEADLINE: u16 = 13;
pub const TEXT_BODY: u16 = 13;
pub const TEXT_CALLOUT: u16 = 12;
pub const TEXT_SUBHEADLINE: u16 = 12;
pub const TEXT_FOOTNOTE: u16 = 11;
pub const TEXT_CAPTION: u16 = 11;
pub const TEXT_CAPTION2: u16 = 10;
pub const TEXT_MICRO: u16 = 9;

// Corner radii per surface (macOS uses continuous corners; iced is circular).
#[allow(dead_code)]
pub const R_COPY: f32 = 4.0;
#[allow(dead_code)]
pub const R_CHIP: f32 = 5.0;
pub const R_SM: f32 = 6.0;
pub const R_MD: f32 = 8.0;
pub const R_LG: f32 = 10.0;
pub const R_PANEL: f32 = 12.0;
#[allow(dead_code)]
pub const R_OVERLAY: f32 = 16.0;

/// Bundled Inter (SF-metric-like). Loaded in `main.rs` via `.font(..)`.
#[allow(dead_code)]
pub fn font_regular() -> Font {
    Font::with_name("Inter")
}

pub fn font_medium() -> Font {
    Font {
        weight: iced::font::Weight::Medium,
        ..Font::with_name("Inter")
    }
}

pub fn font_semibold() -> Font {
    Font {
        weight: iced::font::Weight::Semibold,
        ..Font::with_name("Inter")
    }
}

pub fn font_bold() -> Font {
    Font {
        weight: iced::font::Weight::Bold,
        ..Font::with_name("Inter")
    }
}

/// Numeric text: medium Inter. iced 0.13 exposes no OpenType `tnum` toggle,
/// so callers pair this with fixed-width slots where digits must not jitter.
pub fn font_numeric() -> Font {
    font_medium()
}

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

/// Prominent button (mac `.borderedProminent`): accent fill, white text.
#[allow(dead_code)]
pub fn prominent(_theme: &Theme, status: button::Status) -> button::Style {
    let fill = match status {
        button::Status::Hovered => Color::from_rgb(0.2, 0.58, 1.0),
        button::Status::Pressed => Color::from_rgb(0.0, 0.45, 0.9),
        button::Status::Disabled => Color::from_rgba(0.039, 0.518, 1.0, 0.4),
        button::Status::Active => ACCENT,
    };
    button::Style {
        background: Some(Background::Color(fill)),
        text_color: Color::WHITE,
        border: Border::default().rounded(R_SM),
        ..Default::default()
    }
}

/// Plain/link button (mac `.plain`/`.link`): no plate, subtle hover wash.
pub fn plain(theme: &Theme, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: hovered.then(|| Background::Color(white(0.07))),
        text_color: theme.palette().text,
        border: Border::default().rounded(R_SM),
        ..Default::default()
    }
}

/// Segmented-picker segment (macOS 26 segmented control, measured on the Mac): faint track,
/// accent-filled selected segment with white text, primary-coloured unselected labels.
pub fn segment(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        if selected {
            button::Style {
                background: Some(Background::Color(if hovered {
                    Color::from_rgb(0.2, 0.58, 1.0)
                } else {
                    ACCENT
                })),
                text_color: Color::WHITE,
                border: Border::default().rounded(R_SM),
                ..Default::default()
            }
        } else {
            button::Style {
                background: hovered.then(|| Background::Color(white(0.07))),
                text_color: theme.palette().text,
                border: Border::default().rounded(R_SM),
                ..Default::default()
            }
        }
    }
}

/// Source-list row: neutral gray selection like the Mac Settings sidebar (measured white 0.10).
pub fn sidebar(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let text = theme.palette().text;
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let (background, text_color) = if selected {
            (Some(Background::Color(white(0.10))), text)
        } else if hovered {
            (Some(Background::Color(white(0.07))), text)
        } else {
            (None, text)
        };
        button::Style {
            background,
            text_color,
            border: Border::default().rounded(R_SM),
            ..Default::default()
        }
    }
}

/// Rounded card with a subtle fill and hairline border.
pub fn card(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(white(0.05))),
        border: Border {
            color: SEPARATOR,
            width: 1.0,
            radius: R_LG.into(),
        },
        ..Default::default()
    }
}

/// Spend dashboard panel (mac `SpendDashboardPanel`, radius 12).
pub fn panel(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(white(0.05))),
        border: Border {
            color: SEPARATOR,
            width: 1.0,
            radius: R_PANEL.into(),
        },
        ..Default::default()
    }
}


/// Thin capsule meter: mac track is `tertiaryLabelColor` at 0.22.
pub fn meter(fill: Color, height: f32) -> impl Fn(&Theme) -> progress_bar::Style {
    move |_| progress_bar::Style {
        background: Background::Color(white(0.22)),
        bar: Background::Color(fill),
        border: Border::default().rounded(height / 2.0),
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

/// Mac Form popup button: no field chrome at rest, a soft wash on hover/open.
pub fn mac_popup(theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let fill = match status {
        pick_list::Status::Active => Color::TRANSPARENT,
        pick_list::Status::Hovered => white(0.07),
        pick_list::Status::Opened => white(0.12),
    };
    pick_list::Style {
        text_color: theme.palette().text,
        placeholder_color: white(0.35),
        handle_color: SECONDARY,
        background: Background::Color(fill),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
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
#[allow(dead_code)]
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
