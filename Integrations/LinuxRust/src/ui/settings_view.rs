//! Settings window: searchable sidebar (app panes + provider catalog) and a
//! detail pane per `SettingsPane`, mirroring the macOS Preferences window
//! (audit 5-settings, synthesis screens 4-16) and the Qt
//! `qml/SettingsWindow.qml` controls over the shared `linux.json` settings.
//! Every control clones `App.settings`, mutates the clone, and emits
//! `Message::SettingsChanged`; persistence happens via the Save button
//! (`Message::SaveSettings`), like Qt.

use crate::settings::Settings;
use crate::state::{App, Message, Page, SettingsPane};
use iced::widget::{
    button, checkbox, column, container, radio, row, scrollable, slider, text, text_input,
};
use iced::{Color, Element, Length};
use std::path::PathBuf;

pub(crate) const ALL_PANES: [SettingsPane; 11] = [
    SettingsPane::General,
    SettingsPane::Providers,
    SettingsPane::Spend,
    SettingsPane::Menu,
    SettingsPane::MenuBar,
    SettingsPane::Notifications,
    SettingsPane::Hooks,
    SettingsPane::Plugins,
    SettingsPane::Advanced,
    SettingsPane::Debug,
    SettingsPane::About,
];

const PROVIDER_PRESETS: [&str; 6] = ["codex", "claude", "both", "all", "enabled", "custom"];
const SOURCE_OPTIONS: [(&str, &str); 5] = [
    ("Auto", "auto"),
    ("OAuth", "oauth"),
    ("CLI", "cli"),
    ("API", "api"),
    ("Web", "web"),
];

// ---------- pure helpers ----------

pub fn pane_title(pane: SettingsPane) -> &'static str {
    match pane {
        SettingsPane::General => "General",
        SettingsPane::Providers => "Providers",
        SettingsPane::Spend => "Usage & Spend",
        SettingsPane::Menu => "Menu",
        SettingsPane::MenuBar => "Menu Bar",
        SettingsPane::Notifications => "Notifications",
        SettingsPane::Hooks => "Hooks",
        SettingsPane::Plugins => "Plugins",
        SettingsPane::Advanced => "Advanced",
        SettingsPane::Debug => "Debug",
        SettingsPane::About => "About",
    }
}

pub fn pane_matches(pane: SettingsPane, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    q.is_empty() || pane_title(pane).to_lowercase().contains(&q)
}

pub fn filtered_panes(query: &str) -> Vec<SettingsPane> {
    ALL_PANES
        .into_iter()
        .filter(|p| pane_matches(*p, query))
        .collect()
}

pub fn parse_provider_order(s: &str) -> Vec<String> {
    // Provider ids never contain spaces, so commas and whitespace both
    // separate entries; this keeps the controlled text field typeable even
    // though its value is re-formatted on every keystroke.
    let mut out: Vec<String> = Vec::new();
    for part in s.split(|c: char| c == ',' || c.is_whitespace()) {
        if !part.is_empty() && !out.iter().any(|x| x == part) {
            out.push(part.to_string());
        }
    }
    out
}

pub fn format_provider_order(order: &[String]) -> String {
    order.join(", ")
}

pub fn filtered_catalog<'a>(
    catalog: &'a [(String, String)],
    query: &str,
) -> Vec<&'a (String, String)> {
    let q = query.trim().to_lowercase();
    catalog
        .iter()
        .filter(|(id, name)| {
            q.is_empty() || id.to_lowercase().contains(&q) || name.to_lowercase().contains(&q)
        })
        .collect()
}

pub fn dashboard_url(provider: &str) -> Option<&'static str> {
    match provider {
        "codex" => Some("https://chatgpt.com/codex/settings/usage"),
        "claude" => Some("https://claude.ai/settings/usage"),
        _ => None,
    }
}

pub fn refresh_label(secs: u32) -> String {
    if secs % 60 == 0 {
        format!("{} min", secs / 60)
    } else {
        format!("{secs} sec")
    }
}

/// Unknown values render as an em dash, never 0/empty.
pub fn or_dash(s: &str) -> &str {
    if s.trim().is_empty() {
        "—"
    } else {
        s
    }
}

pub fn plugins_dir() -> PathBuf {
    Settings::path()
        .parent()
        .map(|p| p.join("providers"))
        .unwrap_or_else(|| PathBuf::from("providers"))
}

pub fn config_json_path() -> PathBuf {
    Settings::path()
        .parent()
        .map(|p| p.join("config.json"))
        .unwrap_or_else(|| PathBuf::from("config.json"))
}

pub fn list_plugins() -> Vec<String> {
    let mut names: Vec<String> = match std::fs::read_dir(plugins_dir()) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect(),
        Err(_) => Vec::new(),
    };
    names.sort();
    names
}

/// macOS-style source list row: plain text, accent fill only on the selected row.
fn sidebar_style(selected: bool) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |theme, status| {
        let mut style = if selected {
            button::primary(theme, status)
        } else {
            button::text(theme, status)
        };
        if !selected && matches!(status, button::Status::Hovered) {
            style.background = Some(Color::from_rgba(1.0, 1.0, 1.0, 0.06).into());
        }
        style.border = iced::Border::default().rounded(6.0);
        style
    }
}

/// Status dot drawn as a shape (the default font has no U+25CF glyph).
fn status_dot<'a>(c: Color) -> Element<'a, Message> {
    let dot = container(iced::widget::Space::new(
        Length::Fixed(8.0),
        Length::Fixed(8.0),
    ))
    .style(move |_| container::Style {
        background: Some(c.into()),
        border: iced::Border::default().rounded(4.0),
        ..Default::default()
    });
    container(dot).center_y(Length::Fixed(20.0)).into()
}

fn dot_color(app: &App, provider_id: &str) -> Color {
    match app.entries.iter().find(|e| e.provider == provider_id) {
        Some(e) if !e.error.trim().is_empty() => Color::from_rgb(0.85, 0.30, 0.25),
        Some(_) => Color::from_rgb(0.30, 0.75, 0.40),
        None => Color::from_rgb(0.55, 0.55, 0.55),
    }
}

// ---------- control helpers (clone Settings -> SettingsChanged) ----------

fn changed(app: &App, f: impl Fn(&mut Settings)) -> Message {
    let mut s = app.settings.clone();
    f(&mut s);
    Message::SettingsChanged(Box::new(s))
}

fn check<'a>(
    label: &str,
    cur: bool,
    base: Settings,
    set: impl Fn(&mut Settings, bool) + 'a,
) -> Element<'a, Message> {
    checkbox(label.to_string(), cur)
        .on_toggle(move |v| {
            let mut s = base.clone();
            set(&mut s, v);
            Message::SettingsChanged(Box::new(s))
        })
        .into()
}

fn text_field<'a>(
    placeholder: &'a str,
    cur: &'a str,
    base: Settings,
    set: impl Fn(&mut Settings, String) + 'a,
) -> Element<'a, Message> {
    text_input(placeholder, cur)
        .on_input(move |v| {
            let mut s = base.clone();
            set(&mut s, v);
            Message::SettingsChanged(Box::new(s))
        })
        .into()
}

/// The order field's value is derived (formatted from `provider_order`), so
/// it has no storage in `App`; cache the last formatted string. This leaks
/// one small string per distinct value typed — a draft field in `App` state
/// would remove the need (noted as a contract change for the owner).
fn order_text(order: &[String]) -> &'static str {
    thread_local! {
        static CACHE: std::cell::RefCell<&'static str> = const { std::cell::RefCell::new("") };
    }
    let formatted = format_provider_order(order);
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if *c != formatted.as_str() {
            *c = Box::leak(formatted.into_boxed_str());
        }
        *c
    })
}

fn radio_group<'a>(
    options: &[(&'static str, &'static str)],
    cur: &'a str,
    base: Settings,
    set: fn(&mut Settings, &str),
) -> Element<'a, Message> {
    let mut col = column![].spacing(4);
    for (label, value) in options {
        let b = base.clone();
        col = col.push(radio(*label, *value, Some(cur), move |v: &'a str| {
            let mut s = b.clone();
            set(&mut s, v);
            Message::SettingsChanged(Box::new(s))
        }));
    }
    col.into()
}

fn section<'a>(title: &str, body: Element<'a, Message>) -> Element<'a, Message> {
    column![text(title.to_string()).size(16), body]
        .spacing(8)
        .into()
}

fn note(s: &str) -> Element<'_, Message> {
    text(s.to_string())
        .size(12)
        .color(Color::from_rgb(0.65, 0.65, 0.65))
        .into()
}

fn labeled<'a>(label: &str, control: Element<'a, Message>) -> Element<'a, Message> {
    row![
        container(text(label.to_string())).width(Length::Fixed(220.0)),
        control
    ]
    .spacing(8)
    .into()
}

// ---------- view ----------

pub fn view(app: &App) -> Element<Message> {
    let detail = match app.settings_pane {
        SettingsPane::General => general_pane(app),
        SettingsPane::Providers => providers_pane(app),
        SettingsPane::Spend => spend_pane(app),
        SettingsPane::Menu => menu_pane(app),
        SettingsPane::MenuBar => menu_bar_pane(app),
        SettingsPane::Notifications => notifications_pane(app),
        SettingsPane::Hooks => hooks_pane(app),
        SettingsPane::Plugins => plugins_pane(app),
        SettingsPane::Advanced => advanced_pane(app),
        SettingsPane::Debug => debug_pane(app),
        SettingsPane::About => about_pane(app),
    };
    let mut detail_col = column![text(pane_title(app.settings_pane)).size(20), detail].spacing(12);
    if !app.error.trim().is_empty() {
        detail_col =
            detail_col.push(text(app.error.clone()).color(Color::from_rgb(0.9, 0.4, 0.35)));
    }
    detail_col = detail_col.push(
        row![
            container(note("Closing windows keeps CodexBar running.")).width(Length::Fill),
            button(text("Save")).on_press(Message::SaveSettings),
        ]
        .spacing(8),
    );
    row![
        container(sidebar(app))
            .width(Length::Fixed(260.0))
            .height(Length::Fill),
        container(scrollable(
            container(detail_col).padding(16).width(Length::Fill)
        ))
        .width(Length::Fill)
        .height(Length::Fill),
    ]
    .into()
}

fn sidebar(app: &App) -> Element<'_, Message> {
    let mut col = column![text_input("Search settings", &app.settings_search)
        .on_input(Message::SettingsSearchChanged),]
    .spacing(6)
    .padding(10);
    for pane in filtered_panes(&app.settings_search) {
        let selected = pane == app.settings_pane && app.selected_provider.is_none();
        col = col.push(
            button(text(pane_title(pane)))
                .on_press(Message::OpenSettingsPane(pane))
                .style(sidebar_style(selected))
                .width(Length::Fill),
        );
    }
    col = col.push(text(format!("Providers ({})", app.catalog.len())).size(13));
    let providers = filtered_catalog(&app.catalog, &app.settings_search);
    if providers.is_empty() {
        col = col.push(note(if app.catalog.is_empty() {
            "No providers loaded yet — the catalog loads from the CLI."
        } else {
            "No matching settings or providers."
        }));
    }
    let mut list = column![].spacing(2);
    for (id, name) in providers {
        let selected = app.selected_provider.as_deref() == Some(id.as_str());
        list = list.push(
            button(row![status_dot(dot_color(app, id)), text(name.clone())].spacing(6))
                .on_press(Message::SelectProvider(id.clone()))
                .style(sidebar_style(selected))
                .width(Length::Fill),
        );
    }
    col = col.push(scrollable(list));
    col.into()
}

fn general_pane(app: &App) -> Element<'_, Message> {
    let s = &app.settings;
    let mut presets = row![].spacing(4);
    for p in PROVIDER_PRESETS {
        presets = presets
            .push(button(text(p)).on_press(changed(app, move |st| st.provider = p.to_string())));
    }
    let refresh_slider = {
        let base = s.clone();
        slider(60..=3600, s.refresh_seconds, move |v| {
            let mut st = base.clone();
            st.refresh_seconds = v;
            Message::SettingsChanged(Box::new(st))
        })
        .step(60u32)
    };
    column![
        section(
            "CLI",
            column![
                labeled(
                    "CodexBar CLI executable",
                    text_field("codexbar", &s.executable, s.clone(), |st, v| st.executable = v),
                ),
                labeled(
                    "Provider",
                    text_field("codex", &s.provider, s.clone(), |st, v| st.provider = v),
                ),
                presets,
                note("Choose custom to select and order providers (Providers pane), or enabled to follow your CLI configuration."),
                labeled("Source", radio_group(&SOURCE_OPTIONS, &s.source, s.clone(), |st, v| st.source = v.to_string())),
            ]
            .spacing(8)
            .into(),
        ),
        section(
            "Refreshing",
            column![
                labeled(
                    "Refresh interval",
                    row![container(refresh_slider).width(Length::Fixed(260.0)), text(refresh_label(s.refresh_seconds))]
                        .spacing(8)
                        .into(),
                ),
                check("Refresh when opening usage", s.refresh_on_open, s.clone(), |st, v| st.refresh_on_open = v),
            ]
            .spacing(8)
            .into(),
        ),
        section(
            "Display",
            column![
                check("Show account identity", s.show_identity, s.clone(), |st, v| st.show_identity = v),
                check("Service status", s.show_status, s.clone(), |st, v| st.show_status = v),
                check("Local spending", s.show_costs, s.clone(), |st, v| st.show_costs = v),
            ]
            .spacing(8)
            .into(),
        ),
        section(
            "System",
            column![checkbox("Start at login".to_string(), crate::autostart::status()).on_toggle(Message::SetAutostart)]
                .spacing(8)
                .into(),
        ),
    ]
    .spacing(16)
    .into()
}

fn providers_pane(app: &App) -> Element<'_, Message> {
    let s = &app.settings;
    let selected_id = app
        .selected_provider
        .clone()
        .unwrap_or_else(|| s.provider.clone());
    let order_field = {
        let base = s.clone();
        text_input("codex, claude", order_text(&s.provider_order)).on_input(move |v| {
            let mut st = base.clone();
            st.provider_order = parse_provider_order(&v);
            Message::SettingsChanged(Box::new(st))
        })
    };
    let account_slider = {
        let base = s.clone();
        slider(0..=999, s.account_index, move |v| {
            let mut st = base.clone();
            st.account_index = v;
            Message::SettingsChanged(Box::new(st))
        })
        .step(1u32)
    };
    let mut col = column![section(
        "Provider order",
        column![
            labeled("Order (comma-separated)", order_field.into()),
            note("Used when the provider is set to custom; also orders bars and cards."),
            labeled(
                "Account (0 = default)",
                row![
                    container(account_slider).width(Length::Fixed(220.0)),
                    text(s.account_index.to_string()),
                ]
                .spacing(8)
                .into(),
            ),
            check("All accounts", s.all_accounts, s.clone(), |st, v| st
                .all_accounts =
                v),
        ]
        .spacing(8)
        .into(),
    ),]
    .spacing(16);
    col = col.push(provider_detail(app, &selected_id));
    col.into()
}

fn provider_detail<'a>(app: &'a App, id: &str) -> Element<'a, Message> {
    let name = app
        .catalog
        .iter()
        .find(|(cid, _)| cid == id)
        .map(|(_, n)| n.clone())
        .unwrap_or_else(|| crate::model::provider_name(id));
    let entry = app.entries.iter().find(|e| e.provider == id);
    let mut body =
        column![row![status_dot(dot_color(app, id)), text(name).size(16)].spacing(6),].spacing(6);
    match entry {
        Some(e) => {
            body = body.push(text(format!("Account: {}", or_dash(&e.account_label))));
            body = body.push(text(format!("Plan: {}", or_dash(&e.plan))));
            body = body.push(text(format!("Status: {}", or_dash(&e.status))));
            if !e.error.trim().is_empty() {
                body = body.push(
                    text(format!("Error: {}", e.error)).color(Color::from_rgb(0.9, 0.4, 0.35)),
                );
            }
            body = body.push(text(format!("Updated: {}", or_dash(&e.updated_at))));
            let credits = e
                .credits
                .map(|c| format!("{c}"))
                .unwrap_or_else(|| "—".to_string());
            body = body.push(text(format!("Credits: {credits}")));
            if e.windows.is_empty() {
                body = body.push(text("Usage windows: —"));
            }
            for w in &e.windows {
                let reset = or_dash(&w.reset_text);
                body = body.push(text(format!(
                    "{}: {}% left · {reset}",
                    w.label, w.remaining
                )));
            }
        }
        None => {
            body = body.push(note("No usage data for this provider yet — values show as — until a refresh returns data."));
            body = body.push(text("Plan: —"));
            body = body.push(text("Status: —"));
            body = body.push(text("Usage windows: —"));
        }
    }
    if let Some(meta) = app.provider_meta.iter().find(|p| p.provider == id) {
        let pid = id.to_string();
        body = body.push(
            checkbox("Enabled", meta.enabled)
                .on_toggle(move |v| Message::SetProviderEnabled(pid.clone(), v)),
        );
        let auth = &meta.authentication;
        if auth.api_key || auth.cookie_header {
            let placeholder = if auth.api_key {
                "API key"
            } else {
                "Cookie header"
            };
            let mut actions = row![].spacing(8);
            if auth.api_key {
                actions = actions.push(button(text("Save API key")).on_press_maybe(
                    (!app.credential_draft.trim().is_empty()).then(|| Message::SaveCredential {
                        provider: id.to_string(),
                        cookie: false,
                    }),
                ));
            }
            if auth.cookie_header {
                actions = actions.push(button(text("Save cookie header")).on_press_maybe(
                    (!app.credential_draft.trim().is_empty()).then(|| Message::SaveCredential {
                        provider: id.to_string(),
                        cookie: true,
                    }),
                ));
            }
            actions = actions.push(
                button(text("Clear stored credential"))
                    .style(button::secondary)
                    .on_press(Message::ClearCredential(id.to_string())),
            );
            body = body.push(
                text_input(placeholder, &app.credential_draft)
                    .secure(true)
                    .on_input(Message::CredentialDraftChanged),
            );
            body = body.push(actions);
            body = body.push(note("Stored in the shared CodexBar config via the CLI (sent over stdin, never on the command line)."));
        }
    }
    match &app.config_status {
        Ok(msg) if !msg.is_empty() => body = body.push(note(msg)),
        Err(e) => {
            body = body.push(
                text(e.clone())
                    .size(12)
                    .color(Color::from_rgb(0.9, 0.4, 0.35)),
            )
        }
        _ => {}
    }
    if id == "codex" || id == "claude" {
        let pid = id.to_string();
        body = body.push(
            row![
                button(text("Sign in…")).on_press(Message::AccountAction {
                    provider: pid.clone(),
                    login: true
                }),
                button(text("Sign out…")).on_press(Message::AccountAction {
                    provider: pid,
                    login: false
                }),
            ]
            .spacing(8),
        );
        body = body.push(note("Sign-in opens your provider CLI in a terminal. Account selection only changes displayed usage."));
    }
    if let Some(url) = dashboard_url(id) {
        body = body
            .push(button(text("Open usage dashboard")).on_press(Message::OpenUrl(url.to_string())));
    }
    body = body.push(note(
        "Browser-cookie import works only on macOS; on Linux use the provider CLI sign-in or an API key in the CLI config.",
    ));
    section("Selected provider", body.into())
}

fn spend_pane(app: &App) -> Element<'_, Message> {
    let s = &app.settings;
    column![
        check("Local spending (show costs)", s.show_costs, s.clone(), |st, v| st.show_costs = v),
        note("Spend figures are local list-price estimates from the CodexBar CLI cost scan — not a billing receipt. Unknown values show as —, never 0."),
        button(text("Open Usage & Spend")).on_press(Message::ShowPage(Page::Usage)),
    ]
    .spacing(10)
    .into()
}

fn menu_pane(app: &App) -> Element<'_, Message> {
    let s = &app.settings;
    column![section(
        "Usage",
        column![
            labeled(
                "Quota display",
                radio_group(
                    &[("Remaining", "remaining"), ("Used", "used")],
                    &s.quota_display,
                    s.clone(),
                    |st, v| st.quota_display = v.to_string(),
                ),
            ),
            labeled(
                "Reset time display",
                radio_group(
                    &[
                        ("Countdown", "countdown"),
                        ("Absolute time", "absolute"),
                        ("Both", "both")
                    ],
                    &s.reset_display,
                    s.clone(),
                    |st, v| st.reset_display = v.to_string(),
                ),
            ),
            check("Show pace", s.show_pace, s.clone(), |st, v| st.show_pace =
                v),
        ]
        .spacing(8)
        .into(),
    ),]
    .spacing(16)
    .into()
}

fn menu_bar_pane(app: &App) -> Element<'_, Message> {
    let s = &app.settings;
    let bar_slider = {
        let base = s.clone();
        slider(0..=80, s.bar_providers, move |v| {
            let mut st = base.clone();
            st.bar_providers = v;
            Message::SettingsChanged(Box::new(st))
        })
        .step(1u32)
    };
    column![
        crate::ui::menubar_editor::view(app),
        section(
            "Tray / bar",
            column![
                labeled(
                    "Tray style",
                    radio_group(
                        &[("Meters", "meters"), ("Icon", "icon")],
                        &s.tray_style,
                        s.clone(),
                        |st, v| st.tray_style = v.to_string(),
                    ),
                ),
                check("Tray icon", s.show_tray, s.clone(), |st, v| st.show_tray =
                    v),
                check(
                    "Use compact Quick View from the tray",
                    s.compact_quick_view,
                    s.clone(),
                    |st, v| st.compact_quick_view = v
                ),
                check(
                    "Highlight low quota (warning colors)",
                    s.warning_colors,
                    s.clone(),
                    |st, v| st.warning_colors = v
                ),
                check(
                    "Show session, weekly and pace in the bar",
                    s.show_bar_detail,
                    s.clone(),
                    |st, v| st.show_bar_detail = v
                ),
                check(
                    "Show per-model caps in the bar",
                    s.show_scoped_caps,
                    s.clone(),
                    |st, v| st.show_scoped_caps = v
                ),
                labeled(
                    "Providers in the bar",
                    row![
                        container(bar_slider).width(Length::Fixed(220.0)),
                        text(if s.bar_providers == 0 {
                            "—".to_string()
                        } else {
                            s.bar_providers.to_string()
                        }),
                    ]
                    .spacing(8)
                    .into(),
                ),
                check(
                    "Follow Omarchy theme colors",
                    s.follow_omarchy_theme,
                    s.clone(),
                    |st, v| st.follow_omarchy_theme = v
                ),
            ]
            .spacing(8)
            .into(),
        ),
    ]
    .spacing(16)
    .into()
}

fn notifications_pane(app: &App) -> Element<'_, Message> {
    let s = &app.settings;
    let threshold_slider = {
        let base = s.clone();
        slider(1..=99, s.notify_threshold, move |v| {
            let mut st = base.clone();
            st.notify_threshold = v;
            Message::SettingsChanged(Box::new(st))
        })
        .step(1u32)
    };
    column![
        section(
            "Alerts",
            column![
                check("Notify about low quota, resets, and outages", s.notifications, s.clone(), |st, v| st.notifications = v),
                labeled(
                    "Remaining quota threshold (%)",
                    row![
                        container(threshold_slider).width(Length::Fixed(220.0)),
                        text(format!("{}%", s.notify_threshold)),
                    ]
                    .spacing(8)
                    .into(),
                ),
                note("Delivered through the desktop notification service (org.freedesktop.Notifications)."),
            ]
            .spacing(8)
            .into(),
        ),
    ]
    .spacing(16)
    .into()
}

fn hooks_pane(_app: &App) -> Element<'_, Message> {
    column![
        section(
            "Hooks",
            column![
                note("Hooks run a local executable when quota events happen (quota low/reached/reset, usage updated, provider unavailable/recovered, refresh failed). Only enable hooks you trust — they execute commands on this machine."),
                text(format!("Config file: {}", config_json_path().display())),
                note("Hooks live in config.json (shared with the CLI), not in linux.json, so they are managed from the command line:"),
                text("codexbar hooks list | enable | disable | test | watch"),
            ]
            .spacing(8)
            .into(),
        ),
    ]
    .spacing(16)
    .into()
}

fn plugins_pane(_app: &App) -> Element<'_, Message> {
    let dir = plugins_dir();
    let plugins = list_plugins();
    let mut body = column![
        text(format!("Plugins directory: {}", dir.display())),
        note("Drop a .js or .ts provider plugin in this directory. Plugins run only after approval in the app; approval records the network origins, capabilities, and cookie domains the plugin requested."),
    ]
    .spacing(6);
    if plugins.is_empty() {
        body = body.push(text("No plugins installed."));
    } else {
        for name in &plugins {
            body = body.push(text(name.clone()));
        }
    }
    column![section("Provider plugins", body.into())]
        .spacing(16)
        .into()
}

fn advanced_pane(app: &App) -> Element<'_, Message> {
    let _ = app;
    column![
        section(
            "HTTP server / dashboard",
            column![
                note("The CLI provides the web surfaces: `codexbar serve` runs the dashboard server (Bearer token required off-loopback) and `codexbar dashboard` prints a one-shot JSON snapshot. This desktop app does not embed its own HTTP server."),
                button(text("Copy usage summary")).on_press(Message::CopySummary),
            ]
            .spacing(8)
            .into(),
        ),
        section(
            "Desktop IPC",
            column![
                text(format!("Socket: {}", crate::ipc::socket_path().display())),
                note("Other CodexBar surfaces (tray, Omarchy bar, `codexbar-linux --snapshot`) talk to the running app over this socket."),
            ]
            .spacing(8)
            .into(),
        ),
    ]
    .spacing(16)
    .into()
}

fn debug_pane(app: &App) -> Element<'_, Message> {
    let errors = app
        .entries
        .iter()
        .filter(|e| !e.error.trim().is_empty())
        .count();
    // Leaked like order_text: a read-only text_input needs a borrowed value,
    // and the formatted path has no storage in App. The path is constant per
    // process, so this caches exactly one string.
    let settings_path: &'static str = {
        thread_local! {
            static CACHE: std::cell::RefCell<&'static str> = const { std::cell::RefCell::new("") };
        }
        let formatted = Settings::path().display().to_string();
        CACHE.with(|c| {
            let mut c = c.borrow_mut();
            if *c != formatted.as_str() {
                *c = Box::leak(formatted.into_boxed_str());
            }
            *c
        })
    };
    column![section(
        "Diagnostics",
        column![
            text(format!("Usage entries loaded: {}", app.entries.len())),
            text(format!(
                "Entries with errors: {}",
                if app.entries.is_empty() {
                    "—".to_string()
                } else {
                    errors.to_string()
                }
            )),
            text(format!("Last error: {}", or_dash(&app.error))),
            text(format!("Cost error: {}", or_dash(&app.cost_error))),
            text(format!(
                "CLI executable: {}",
                or_dash(&app.settings.executable)
            )),
            labeled(
                "Settings file (copyable)",
                text_input("", &settings_path).into()
            ),
            text(format!("Catalog providers: {}", app.catalog.len())),
            text(format!(
                "Busy: {} · Cost busy: {} · Stale: {}",
                app.busy, app.cost_busy, app.stale
            )),
            text(format!("Last updated: {}", or_dash(&app.updated))),
            note("Run `codexbar diagnose --format json` for a full redacted diagnostic export."),
        ]
        .spacing(6)
        .into(),
    ),]
    .spacing(16)
    .into()
}

fn about_pane(_app: &App) -> Element<'_, Message> {
    column![
        text("CodexBar Extra").size(18),
        text(format!("Version {}", env!("CARGO_PKG_VERSION"))),
        note("Usage and spend for Codex, Claude, and other AI coding providers — Linux desktop build."),
        button(text("GitHub — this fork (FisiFla/codexbar-extra)"))
            .on_press(Message::OpenUrl("https://github.com/FisiFla/codexbar-extra".to_string())),
        button(text("GitHub — upstream (steipete/CodexBar)"))
            .on_press(Message::OpenUrl("https://github.com/steipete/CodexBar".to_string())),
        button(text("Website — codexbar.app"))
            .on_press(Message::OpenUrl("https://codexbar.app".to_string())),
        text("Updates unavailable in this build."),
        note("Update through your package source or by rebuilding from the fork."),
    ]
    .spacing(8)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_filter_empty_query_shows_all() {
        assert_eq!(filtered_panes("").len(), 11);
        assert_eq!(filtered_panes("  ").len(), 11);
    }

    #[test]
    fn pane_filter_matches_title_case_insensitive() {
        assert!(pane_matches(SettingsPane::Notifications, "NOTIF"));
        assert!(pane_matches(SettingsPane::MenuBar, "menu bar"));
        assert!(!pane_matches(SettingsPane::General, "hooks"));
        assert_eq!(
            filtered_panes("menu"),
            vec![SettingsPane::Menu, SettingsPane::MenuBar]
        );
    }

    #[test]
    fn pane_titles_cover_sidebar() {
        assert_eq!(pane_title(SettingsPane::General), "General");
        assert_eq!(pane_title(SettingsPane::Providers), "Providers");
        assert_eq!(pane_title(SettingsPane::Spend), "Usage & Spend");
        assert_eq!(pane_title(SettingsPane::MenuBar), "Menu Bar");
        assert_eq!(pane_title(SettingsPane::About), "About");
    }

    #[test]
    fn provider_order_parse_trims_dedupes_drops_empty() {
        assert_eq!(
            parse_provider_order("codex, claude,,codex, "),
            vec!["codex", "claude"]
        );
        assert!(parse_provider_order("").is_empty());
        assert!(parse_provider_order(" , ,").is_empty());
        assert_eq!(parse_provider_order("gemini"), vec!["gemini"]);
        assert_eq!(
            parse_provider_order("codex claude"),
            vec!["codex", "claude"]
        );
    }

    #[test]
    fn provider_order_roundtrip() {
        let order = vec!["codex".to_string(), "claude".to_string()];
        assert_eq!(format_provider_order(&order), "codex, claude");
        assert_eq!(format_provider_order(&[]), "");
        assert_eq!(parse_provider_order(&format_provider_order(&order)), order);
    }

    #[test]
    fn catalog_filter_matches_id_and_name() {
        let catalog = vec![
            ("codex".to_string(), "Codex".to_string()),
            ("claude".to_string(), "Claude".to_string()),
            ("gemini".to_string(), "Gemini CLI".to_string()),
        ];
        assert_eq!(filtered_catalog(&catalog, "").len(), 3);
        assert_eq!(filtered_catalog(&catalog, "CLA").len(), 1);
        assert_eq!(filtered_catalog(&catalog, "cli")[0].0, "gemini");
        assert!(filtered_catalog(&catalog, "zzz").is_empty());
    }

    #[test]
    fn dashboard_urls_only_known_providers() {
        assert_eq!(
            dashboard_url("codex"),
            Some("https://chatgpt.com/codex/settings/usage")
        );
        assert_eq!(
            dashboard_url("claude"),
            Some("https://claude.ai/settings/usage")
        );
        assert_eq!(dashboard_url("gemini"), None);
    }

    #[test]
    fn refresh_label_humanizes() {
        assert_eq!(refresh_label(60), "1 min");
        assert_eq!(refresh_label(300), "5 min");
        assert_eq!(refresh_label(90), "90 sec");
        assert_eq!(refresh_label(3600), "60 min");
    }

    #[test]
    fn or_dash_for_unknown() {
        assert_eq!(or_dash(""), "—");
        assert_eq!(or_dash("  "), "—");
        assert_eq!(or_dash("Pro"), "Pro");
    }
}
