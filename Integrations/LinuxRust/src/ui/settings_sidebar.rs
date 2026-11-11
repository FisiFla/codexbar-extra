//! Settings sidebar, mirroring `PreferencesSidebar.swift`: search field plus sort control,
//! fixed app panes with coloured icon tiles, then a "Providers  N on" header and one row per
//! provider (enabled = full brightness + green dot, disabled = dimmed, no dot).

use crate::state::{App, Message, SettingsPane};
use crate::ui::style::{self, ACCENT, SECONDARY};
use iced::widget::{column, container, row, svg, text, Space};
use iced::{Background, Border, Color, Element, Length};

/// Sidebar order, following Swift `appPanes`; Linux-only panes sit next to their equivalents.
pub(crate) const PANE_ORDER: [SettingsPane; 11] = [
    SettingsPane::General,
    SettingsPane::Spend,
    SettingsPane::Notifications,
    SettingsPane::MenuBar,
    SettingsPane::Menu,
    SettingsPane::Providers,
    SettingsPane::Advanced,
    SettingsPane::Hooks,
    SettingsPane::Plugins,
    SettingsPane::About,
    SettingsPane::Debug,
];

pub const SIDEBAR_WIDTH: f32 = 260.0;

/// Whether the provider is on: enabled in the CLI config (`provider_meta`), or currently
/// reporting usage without an error (the "all"/custom provider selections fetch providers the
/// CLI config does not mark enabled).
/// The config's enabled flag decides. Only before the catalog has loaded do providers that
/// returned usage stand in, so a disabled provider with working credentials stays hidden.
pub fn provider_enabled(app: &App, id: &str) -> bool {
    if app.provider_meta.is_empty() {
        return app
            .entries
            .iter()
            .any(|e| e.provider == id && e.error.trim().is_empty());
    }
    app.provider_meta.iter().any(|p| p.provider == id && p.enabled)
}

/// Entries the popover shows (switcher tabs, overview rows): enabled providers only, with
/// their index into `app.entries`.
pub fn shown_entries(app: &App) -> Vec<(usize, &crate::model::Entry)> {
    app.entries
        .iter()
        .enumerate()
        .filter(|(_, e)| provider_enabled(app, &e.provider))
        .collect()
}

pub fn enabled_count(catalog: &[(String, String)], enabled: impl Fn(&str) -> bool) -> usize {
    catalog.iter().filter(|(id, _)| enabled(id)).count()
}

/// Catalog order, or alphabetical with enabled providers first (Swift `alphabeticalProviderOrder`).
pub fn ordered_providers<'a>(
    catalog: &'a [(String, String)],
    alphabetical: bool,
    enabled: impl Fn(&str) -> bool,
) -> Vec<&'a (String, String)> {
    let mut v: Vec<&(String, String)> = catalog.iter().collect();
    if alphabetical {
        v.sort_by(|a, b| {
            enabled(&b.0)
                .cmp(&enabled(&a.0))
                .then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase()))
        });
    }
    v
}

fn semibold() -> iced::Font {
    crate::ui::style::font_semibold()
}

// ---------- pane icon tiles ----------

fn pane_color(p: SettingsPane) -> Color {
    let c = |r: u8, g: u8, b: u8| Color::from_rgb8(r, g, b);
    match p {
        SettingsPane::General => c(142, 142, 147),
        SettingsPane::Spend => c(52, 199, 89),
        SettingsPane::Notifications => c(255, 69, 58),
        SettingsPane::MenuBar => c(10, 132, 255),
        SettingsPane::Menu => c(48, 176, 199),
        SettingsPane::Providers => c(255, 55, 95),
        SettingsPane::Advanced => c(175, 82, 222),
        SettingsPane::Hooks => c(255, 159, 10),
        SettingsPane::Plugins => c(88, 86, 214),
        SettingsPane::About => c(90, 170, 240),
        SettingsPane::Debug => c(255, 69, 58),
    }
}

/// Small white glyphs drawn on a 24x24 grid (original artwork, not Apple assets).
fn pane_glyph(p: SettingsPane) -> &'static str {
    match p {
        SettingsPane::General => {
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="6" fill="none" stroke="#fff" stroke-width="3.2" stroke-dasharray="2.6 1.9"/><circle cx="12" cy="12" r="5" fill="#fff"/><circle cx="12" cy="12" r="2" fill="#8e8e93"/></svg>"##
        }
        SettingsPane::Spend => {
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#fff"><rect x="3.5" y="12" width="4.5" height="8" rx="1.2"/><rect x="9.8" y="5" width="4.5" height="15" rx="1.2"/><rect x="16" y="9" width="4.5" height="11" rx="1.2"/></svg>"##
        }
        SettingsPane::Notifications => {
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#fff"><path d="M12 3.5c-3.3 0-5.6 2.5-5.6 5.8v3.4L4.6 16v1.4h14.8V16l-1.8-3.3V9.3c0-3.3-2.3-5.8-5.6-5.8z"/><path d="M9.8 19h4.4a2.2 2.2 0 0 1-4.4 0z"/></svg>"##
        }
        SettingsPane::MenuBar => {
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><rect x="3.5" y="5" width="17" height="14" rx="2.4" fill="none" stroke="#fff" stroke-width="2"/><rect x="3.5" y="5" width="17" height="4" fill="#fff"/></svg>"##
        }
        SettingsPane::Menu => {
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><rect x="3.5" y="5" width="17" height="14" rx="2.4" fill="none" stroke="#fff" stroke-width="2"/><rect x="7" y="9" width="10" height="2.2" rx="1" fill="#fff"/><rect x="7" y="13" width="6.5" height="2.2" rx="1" fill="#fff"/></svg>"##
        }
        SettingsPane::Providers => {
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#fff"><circle cx="5.5" cy="7" r="1.8"/><circle cx="5.5" cy="12" r="1.8"/><circle cx="5.5" cy="17" r="1.8"/><rect x="9.5" y="6" width="11" height="2" rx="1"/><rect x="9.5" y="11" width="11" height="2" rx="1"/><rect x="9.5" y="16" width="11" height="2" rx="1"/></svg>"##
        }
        SettingsPane::Advanced => {
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" stroke="#fff" stroke-width="2" stroke-linecap="round"><path d="M4 7h16M4 12h16M4 17h16" fill="none"/><circle cx="9" cy="7" r="2.2" fill="#af52de"/><circle cx="15" cy="12" r="2.2" fill="#af52de"/><circle cx="8" cy="17" r="2.2" fill="#af52de"/></svg>"##
        }
        SettingsPane::Hooks => {
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#fff"><path d="M13.5 3 6 13.2h5L10 21l8-10.5h-5z"/></svg>"##
        }
        SettingsPane::Plugins => {
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#fff"><rect x="5" y="9" width="14" height="10" rx="2"/><rect x="8" y="4" width="2.4" height="5"/><rect x="13.6" y="4" width="2.4" height="5"/></svg>"##
        }
        SettingsPane::About => {
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#fff"><path d="M3 7.5A1.5 1.5 0 0 1 4.5 6h5l2 2.2h8A1.5 1.5 0 0 1 21 9.7v8.3a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 18z"/></svg>"##
        }
        SettingsPane::Debug => {
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#fff"><ellipse cx="12" cy="14" rx="5" ry="6.5"/><circle cx="12" cy="6.5" r="2.6"/><path d="M4 11h4M16 11h4M4 17h4M16 17h4" stroke="#fff" stroke-width="1.8" stroke-linecap="round"/></svg>"##
        }
    }
}

/// 20pt rounded-square tile (flat Mac glyph plate) with a white glyph.
pub fn icon_tile<'a>(pane: SettingsPane) -> Element<'a, Message> {
    let base = pane_color(pane);
    let glyph = svg(svg::Handle::from_memory(pane_glyph(pane).as_bytes()))
        .width(Length::Fixed(13.0))
        .height(Length::Fixed(13.0));
    container(glyph)
        .center_x(Length::Fixed(20.0))
        .center_y(Length::Fixed(20.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(base)),
            border: Border::default().rounded(5.0),
            ..Default::default()
        })
        .into()
}

// ---------- view ----------

fn sort_button(alphabetical: bool) -> Element<'static, Message> {
    let glyph = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="{}" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M8 20V5M4 9l4-4 4 4M16 4v15M12 15l4 4 4-4"/></svg>"##,
        if alphabetical { "#0a84ff" } else { "#9e9ea6" }
    );
    style::button(
        svg(svg::Handle::from_memory(glyph.into_bytes()))
            .width(Length::Fixed(16.0))
            .height(Length::Fixed(16.0)),
    )
    .padding([4, 6])
    .style(style::sidebar(false))
    .on_press(Message::ToggleProviderSort)
    .into()
}

fn search_field(app: &App) -> Element<'_, Message> {
    let glyph = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#9e9ea6" stroke-width="2.4" stroke-linecap="round"><circle cx="10.5" cy="10.5" r="6.5"/><path d="M15.5 15.5 21 21"/></svg>"##;
    container(
        row![
            svg(svg::Handle::from_memory(glyph.as_bytes()))
                .width(Length::Fixed(14.0))
                .height(Length::Fixed(14.0)),
            iced::widget::text_input("Search providers", &app.settings_search)
                .on_input(Message::SettingsSearchChanged)
                .style(|_, _| iced::widget::text_input::Style {
                    background: Background::Color(Color::TRANSPARENT),
                    border: Border::default(),
                    icon: Color::WHITE,
                    placeholder: SECONDARY,
                    value: Color::from_rgb(0.95, 0.95, 0.97),
                    selection: ACCENT,
                })
                .padding(0)
                .size(13),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center),
    )
    .padding([5, 8])
    .width(Length::Fill)
    .style(|_| container::Style {
        background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.08))),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.12),
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    })
    .into()
}

fn status_dot<'a>() -> Element<'a, Message> {
    container(Space::new(Length::Fixed(7.0), Length::Fixed(7.0)))
        .style(|_| container::Style {
            background: Some(Color::from_rgb8(48, 209, 88).into()),
            border: Border::default().rounded(4.0),
            ..Default::default()
        })
        .into()
}

pub fn view(app: &App) -> Element<'_, Message> {
    let alphabetical = app.settings_sort_alpha;
    let mut list = column![].spacing(2);

    for pane in crate::ui::settings_view::filtered_panes(&app.settings_search) {
        let selected = pane == app.settings_pane && app.selected_provider.is_none();
        list = list.push(
            style::button(
                row![
                    icon_tile(pane),
                    text(crate::ui::settings_view::pane_title(pane)).size(13)
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            )
            .on_press(Message::OpenSettingsPane(pane))
            .style(style::sidebar(selected))
            .padding([5, 8])
            .width(Length::Fill),
        );
    }

    let on = enabled_count(&app.catalog, |id| provider_enabled(app, id));
    list = list.push(Space::with_height(Length::Fixed(6.0)));
    list = list.push(
        container(iced::widget::horizontal_rule(1)).padding([4.0, 8.0]),
    );
    list = list.push(
        container(
            row![
                text("Providers").size(12).font(semibold()).color(SECONDARY),
                Space::with_width(Length::Fill),
                text(format!("{on} on")).size(12).color(Color { a: 0.7, ..SECONDARY }),
            ]
            .padding([0, 8]),
        )
        .width(Length::Fill),
    );

    let ordered = ordered_providers(&app.catalog, alphabetical, |id| provider_enabled(app, id));
    let q = app.settings_search.trim().to_lowercase();
    let mut shown = 0;
    for (id, name) in ordered {
        if !(q.is_empty() || id.to_lowercase().contains(&q) || name.to_lowercase().contains(&q)) {
            continue;
        }
        shown += 1;
        let enabled = provider_enabled(app, id);
        let selected = app.selected_provider.as_deref() == Some(id.as_str());
        let tint = if selected {
            Color::WHITE
        } else if enabled {
            Color::from_rgb(0.95, 0.95, 0.97)
        } else {
            Color::from_rgba(0.95, 0.95, 0.97, 0.45)
        };
        let mut r = row![
            crate::icons::icon::<Message>(id, 16.0, tint),
            text(name.clone()).size(13).color(tint),
            Space::with_width(Length::Fill),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        if enabled {
            r = r.push(status_dot());
        }
        list = list.push(
            style::button(r)
                .on_press(Message::SelectProvider(id.clone()))
                .style(style::sidebar(selected))
                .padding([5, 8])
                .width(Length::Fill),
        );
    }
    if shown == 0 {
        list = list.push(
            container(
                text(if app.catalog.is_empty() {
                    "No providers loaded yet — the catalog loads from the CLI."
                } else {
                    "No matching providers"
                })
                .size(12)
                .color(SECONDARY),
            )
            .padding([2, 8]),
        );
    }

    container(
        column![
            row![search_field(app), sort_button(alphabetical)]
                .spacing(6)
                .align_y(iced::Alignment::Center)
                .padding([12, 10]),
            style::scrollable(container(list).padding([0, 10])).height(Length::Fill),
        ]
        .spacing(0),
    )
    .width(Length::Fixed(SIDEBAR_WIDTH))
    .height(Length::Fill)
    .style(|_| container::Style {
        // Mac sidebar sits lighter than the content area (measured #2b2928).
        background: Some(Color::from_rgb8(42, 41, 41).into()),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.07),
            width: 0.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    })
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat() -> Vec<(String, String)> {
        vec![
            ("codex".into(), "Codex".into()),
            ("openai".into(), "OpenAI".into()),
            ("claude".into(), "Claude".into()),
            ("azure".into(), "Azure OpenAI".into()),
        ]
    }

    #[test]
    fn pane_order_matches_swift_with_linux_extras() {
        use SettingsPane::*;
        assert_eq!(&PANE_ORDER[..5], &[General, Spend, Notifications, MenuBar, Menu]);
        assert_eq!(PANE_ORDER[9], About);
        assert_eq!(PANE_ORDER[10], Debug);
    }

    #[test]
    fn enabled_count_counts_enabled() {
        let c = cat();
        assert_eq!(enabled_count(&c, |id| id == "openai" || id == "claude"), 2);
        assert_eq!(enabled_count(&[], |_| true), 0);
    }

    #[test]
    fn catalog_order_kept_when_not_alphabetical() {
        let c = cat();
        let ids: Vec<_> = ordered_providers(&c, false, |_| false).iter().map(|p| p.0.as_str()).collect();
        assert_eq!(ids, ["codex", "openai", "claude", "azure"]);
    }

    #[test]
    fn alphabetical_puts_enabled_first() {
        let c = cat();
        let ids: Vec<_> = ordered_providers(&c, true, |id| id == "openai" || id == "claude")
            .iter()
            .map(|p| p.0.as_str())
            .collect();
        assert_eq!(ids, ["claude", "openai", "azure", "codex"]);
    }
}
