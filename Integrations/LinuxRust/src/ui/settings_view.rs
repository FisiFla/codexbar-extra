//! Settings window: searchable sidebar (app panes + provider catalog) and a
//! detail pane per `SettingsPane`, mirroring the macOS Preferences window
//! (audit 5-settings, synthesis screens 4-16) and the Qt
//! `qml/SettingsWindow.qml` controls over the shared `linux.json` settings.
//! Every control clones `App.settings`, mutates the clone, and emits
//! `Message::SettingsChanged`; persistence happens via the Save button
//! (`Message::SaveSettings`), like Qt.

use crate::settings::Settings;
use crate::state::{App, Message, SettingsPane};
use crate::ui::style::scrollable;
use iced::widget::slider;
use iced::widget::{column, container, row, text};
use iced::{Background, Border, Color, Element, Length};
use std::path::PathBuf;

pub(crate) const ALL_PANES: [SettingsPane; 11] = crate::ui::settings_sidebar::PANE_ORDER;

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

#[allow(dead_code)]
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

/// Shared CLI config, resolved like `CodexBarConfigStore.defaultURL`.
pub fn config_json_path() -> PathBuf {
    let env = |k: &str| std::env::var(k).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let expand = |v: String| match v.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(v),
    };
    if let Some(o) = env("CODEXBAR_CONFIG") {
        return expand(o);
    }
    if let Some(x) = env("XDG_CONFIG_HOME").map(expand).filter(|p| p.is_absolute()) {
        return x.join("codexbar").join("config.json");
    }
    let xdg = home.join(".config/codexbar/config.json");
    let legacy = home.join(".codexbar/config.json");
    if !xdg.exists() && legacy.exists() {
        return legacy;
    }
    xdg
}

/// `~/…` form of a path under the home directory (Mac shows paths abbreviated).
fn tilde(p: &std::path::Path) -> String {
    match std::env::var_os("HOME").map(PathBuf::from) {
        Some(h) if p.starts_with(&h) => format!("~/{}", p.strip_prefix(&h).unwrap().display()),
        _ => p.display().to_string(),
    }
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

// ---------- control helpers (clone Settings -> SettingsChanged) ----------

fn switch_row<'a>(
    label: &str,
    cur: bool,
    on_toggle: impl Fn(bool) -> Message + 'a,
) -> Element<'a, Message> {
    row![
        text(label.to_string())
            .size(crate::ui::style::TEXT_BODY)
            .width(Length::Fill),
        iced::widget::toggler(cur)
            .on_toggle(on_toggle)
            .size(22.0)
            .style(crate::ui::style::toggler),
    ]
    .spacing(12)
    .align_y(iced::Alignment::Center)
    .into()
}

fn check<'a>(
    label: &str,
    cur: bool,
    base: Settings,
    set: impl Fn(&mut Settings, bool) + 'a,
) -> Element<'a, Message> {
    switch_row(label, cur, move |v| {
        let mut s = base.clone();
        set(&mut s, v);
        Message::SettingsChanged(Box::new(s))
    })
}

fn text_field<'a>(
    placeholder: &'a str,
    cur: &'a str,
    base: Settings,
    set: impl Fn(&mut Settings, String) + 'a,
) -> Element<'a, Message> {
    crate::ui::style::text_input(placeholder, cur)
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

/// Popup choice (label shown, value stored); `pick_list` needs `Display`.
#[derive(Clone, PartialEq)]
struct Choice<T> {
    label: String,
    value: T,
}

impl<T> std::fmt::Display for Choice<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// Mac Form popup button: plain label with the chevron, no field chrome.
fn popup<'a, T: Clone + PartialEq + 'a>(
    choices: Vec<(String, T)>,
    cur: &T,
    on_pick: impl Fn(T) -> Message + 'a,
) -> Element<'a, Message> {
    let opts: Vec<Choice<T>> = choices
        .into_iter()
        .map(|(label, value)| Choice { label, value })
        .collect();
    let selected = opts.iter().find(|c| &c.value == cur).cloned();
    crate::ui::style::pick_list(opts, selected, move |c: Choice<T>| on_pick(c.value))
        .style(crate::ui::style::mac_popup)
        .into()
}

/// String-valued settings popup; an unknown current value stays listed so it is never lost.
fn settings_popup<'a>(
    options: &[(&str, &str)],
    cur: &str,
    base: Settings,
    set: fn(&mut Settings, &str),
) -> Element<'a, Message> {
    let mut choices: Vec<(String, String)> = options
        .iter()
        .map(|(l, v)| (l.to_string(), v.to_string()))
        .collect();
    if !cur.is_empty() && !choices.iter().any(|(_, v)| v == cur) {
        choices.push((cur.to_string(), cur.to_string()));
    }
    popup(choices, &cur.to_string(), move |v: String| {
        let mut st = base.clone();
        set(&mut st, &v);
        Message::SettingsChanged(Box::new(st))
    })
}

/// Refresh cadence popup (Mac `RefreshFrequency` timed cases; Linux floors at one minute).
fn refresh_popup<'a>(s: &Settings) -> Element<'a, Message> {
    let mut secs = vec![60u32, 120, 300, 900, 1800, 3600];
    if !secs.contains(&s.refresh_seconds) {
        secs.push(s.refresh_seconds);
        secs.sort_unstable();
    }
    let base = s.clone();
    popup(
        secs.into_iter().map(|v| (refresh_label(v), v)).collect(),
        &s.refresh_seconds,
        move |v| {
            let mut st = base.clone();
            st.refresh_seconds = v;
            Message::SettingsChanged(Box::new(st))
        },
    )
}

/// Label left, compact control right (Mac Form row with a popup or stepper).
fn pref<'a>(label: &str, control: Element<'a, Message>) -> Element<'a, Message> {
    row![
        text(label.to_string())
            .size(crate::ui::style::TEXT_BODY)
            .width(Length::Fill),
        control,
    ]
    .spacing(12)
    .align_y(iced::Alignment::Center)
    .into()
}

/// A row with its caption beneath, kept inside the same separator band.
fn with_note<'a>(row: Element<'a, Message>, s: &'a str) -> Element<'a, Message> {
    column![row, note(s)].spacing(3).into()
}

fn hairline<'a>() -> Element<'a, Message> {
    container(iced::widget::Space::new(Length::Fill, Length::Fixed(1.0)))
        .style(|_: &iced::Theme| container::Style {
            background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.07))),
            ..Default::default()
        })
        .into()
}

/// Mac grouped Form section: header, rows split by hairlines, optional footer note.
fn section_form<'a>(
    title: &str,
    items: Vec<Element<'a, Message>>,
    footer: Option<&'a str>,
) -> Element<'a, Message> {
    let n = items.len();
    let mut rows = column![].width(Length::Fill);
    for (i, item) in items.into_iter().enumerate() {
        rows = rows.push(container(item).padding([8, 0]).width(Length::Fill));
        if i + 1 < n {
            rows = rows.push(hairline());
        }
    }
    let mut col = column![].spacing(8).padding([0, 4]);
    if !title.is_empty() {
        col = col.push(
            text(title.to_string())
                .size(crate::ui::style::TEXT_HEADLINE)
                .font(crate::ui::style::font_semibold()),
        );
    }
    col = col.push(
        container(rows)
            .padding([3, 12])
            .width(Length::Fill)
            .style(crate::ui::style::panel),
    );
    if let Some(f) = footer {
        col = col.push(container(note(f)).padding([0, 8]));
    }
    col.into()
}

fn group_box<'a>(body: Element<'a, Message>) -> Element<'a, Message> {
    container(body)
        .padding([12, 16])
        .width(Length::Fill)
        .style(crate::ui::style::panel)
        .into()
}

fn note_owned<'a>(s: String) -> Element<'a, Message> {
    text(s)
        .size(crate::ui::style::TEXT_FOOTNOTE)
        .color(crate::ui::style::SECONDARY)
        .into()
}

fn note(s: &str) -> Element<'_, Message> {
    text(s.to_string())
        .size(crate::ui::style::TEXT_FOOTNOTE)
        .color(crate::ui::style::SECONDARY)
        .into()
}

/// Label left, control right (Mac grouped-form row).
fn labeled<'a>(label: &str, control: Element<'a, Message>) -> Element<'a, Message> {
    row![
        text(label.to_string())
            .size(crate::ui::style::TEXT_BODY)
            .width(Length::Fill),
        container(control).width(Length::Fixed(340.0)),
    ]
    .spacing(12)
    .align_y(iced::Alignment::Center)
    .into()
}

// ---------- view ----------

pub fn view(app: &App) -> Element<'_, Message> {
    let spend = app.settings_pane == SettingsPane::Spend;
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
    let mut detail_col = column![detail].spacing(16);
    if !spend {
        // Mac panes carry no in-pane title (it lives in the window title bar).
        if !app.error.trim().is_empty() {
            detail_col =
                detail_col.push(text(app.error.clone()).color(Color::from_rgb(0.9, 0.4, 0.35)));
        }
        detail_col = detail_col.push(group_box(
            row![
                container(note("Closing windows keeps CodexBar running.")).width(Length::Fill),
                crate::ui::style::button(text("Save")).on_press(Message::SaveSettings),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .into(),
        ));
    }
    let mut pane = scrollable(
        container(detail_col)
            .padding(iced::Padding { top: 6.0, right: 20.0, bottom: 20.0, left: 20.0 })
            .width(Length::Fill),
    );
    if spend {
        pane = pane.id(crate::ui::spend_view::scroll_id());
    }
    row![
        crate::ui::settings_sidebar::view(app),
        column![toolbar(app, spend), container(pane).width(Length::Fill).height(Length::Fill)],
    ]
    .into()
}

/// Mac Settings toolbar: back/forward capsule and the page title (Spend draws its own heading).
fn toolbar(app: &App, spend: bool) -> Element<'_, Message> {
    let arrow = |glyph: &'static str, msg: Message, enabled: bool| {
        crate::ui::style::button(
            text(glyph)
                .size(24)
                .color(if enabled { Color::WHITE } else { Color::from_rgba(1.0, 1.0, 1.0, 0.3) }),
        )
        .on_press_maybe(enabled.then_some(msg))
        .style(crate::ui::style::plain)
        .padding([0, 12])
    };
    let divider = container(iced::widget::Space::new(Length::Fixed(1.0), Length::Fixed(16.0)))
        .style(|_: &iced::Theme| container::Style {
            background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.12))),
            ..Default::default()
        });
    let nav = container(
        row![
            arrow("‹", Message::SettingsBack, !app.settings_back.is_empty()),
            divider,
            arrow("›", Message::SettingsForward, !app.settings_forward.is_empty()),
        ]
        .align_y(iced::Alignment::Center),
    )
    .padding([3, 2])
    .style(|_: &iced::Theme| container::Style {
        background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.08))),
        border: Border::default().rounded(16.0),
        ..Default::default()
    });
    let title = match (&app.selected_provider, spend) {
        (_, true) => String::new(),
        (Some(id), _) => crate::model::provider_name(id),
        (None, _) => pane_title(app.settings_pane).to_string(),
    };
    row![
        nav,
        text(title)
            .size(crate::ui::style::TEXT_TITLE3)
            .font(crate::ui::style::font_semibold()),
    ]
    .spacing(14)
    .align_y(iced::Alignment::Center)
    .padding([10, 20])
    .into()
}

fn general_pane(app: &App) -> Element<'_, Message> {
    let s = &app.settings;
    let provider_choices: Vec<(&str, &str)> = PROVIDER_PRESETS.iter().map(|p| (*p, *p)).collect();
    column![
        section_form(
            "CLI",
            vec![
                labeled(
                    "CodexBar CLI executable",
                    text_field("codexbar", &s.executable, s.clone(), |st, v| st.executable = v),
                ),
                with_note(
                    pref(
                        "Provider",
                        settings_popup(&provider_choices, &s.provider, s.clone(), |st, v| st.provider = v.to_string()),
                    ),
                    "Choose custom to select and order providers (Providers pane), or enabled to follow your CLI configuration.",
                ),
                pref("Source", settings_popup(&SOURCE_OPTIONS, &s.source, s.clone(), |st, v| st.source = v.to_string())),
            ],
            None,
        ),
        section_form(
            "Refreshing",
            vec![
                pref("Refresh interval", refresh_popup(s)),
                check("Refresh when opening usage", s.refresh_on_open, s.clone(), |st, v| st.refresh_on_open = v),
            ],
            None,
        ),
        section_form(
            "Display",
            vec![
                check("Show account identity", s.show_identity, s.clone(), |st, v| st.show_identity = v),
                check("Service status", s.show_status, s.clone(), |st, v| st.show_status = v),
                check("Local spending", s.show_costs, s.clone(), |st, v| st.show_costs = v),
            ],
            None,
        ),
        section_form(
            "System",
            vec![switch_row("Start at login", crate::autostart::status(), Message::SetAutostart)],
            None,
        ),
    ]
    .spacing(20)
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
        crate::ui::style::text_input("codex, claude", order_text(&s.provider_order)).on_input(
            move |v| {
                let mut st = base.clone();
                st.provider_order = parse_provider_order(&v);
                Message::SettingsChanged(Box::new(st))
            },
        )
    };
    let account_slider = {
        let base = s.clone();
        slider(0..=999, s.account_index, move |v| {
            let mut st = base.clone();
            st.account_index = v;
            Message::SettingsChanged(Box::new(st))
        })
        .step(1u32)
        .style(crate::ui::style::slider)
    };
    let mut col = provider_detail(app, &selected_id);
    col = col.push(section_form(
        "Provider order",
        vec![
            with_note(
                pref("Order", container(order_field).width(Length::Fixed(260.0)).into()),
                "Comma-separated. Used when the provider is set to custom; also orders bars and cards.",
            ),
            pref(
                "Account",
                row![
                    container(account_slider).width(Length::Fixed(200.0)),
                    text(if s.account_index == 0 {
                        "Default".to_string()
                    } else {
                        s.account_index.to_string()
                    })
                    .size(crate::ui::style::TEXT_BODY)
                    .color(crate::ui::style::SECONDARY)
                    .width(Length::Fixed(52.0)),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center)
                .into(),
            ),
            check("All accounts", s.all_accounts, s.clone(), |st, v| st.all_accounts = v),
        ],
        None,
    ));
    col.into()
}

/// Mac Form value row: primary label left, secondary value right.
fn detail_row<'a>(label: &str, value: String) -> Element<'a, Message> {
    row![
        text(label.to_string())
            .size(crate::ui::style::TEXT_BODY)
            .width(Length::Fill),
        text(value)
            .size(crate::ui::style::TEXT_BODY)
            .color(crate::ui::style::SECONDARY),
    ]
    .spacing(12)
    .align_y(iced::Alignment::Center)
    .into()
}

/// "47 sec ago" for an RFC 3339 timestamp; anything else is shown as given.
fn updated_text(raw: &str) -> String {
    match chrono::DateTime::parse_from_rfc3339(raw.trim()) {
        Ok(t) => crate::ui::popover::relative_ago(t.timestamp(), chrono::Utc::now().timestamp()),
        Err(_) => or_dash(raw).to_string(),
    }
}

/// Mac provider detail: header card with value rows, then Usage, Credentials and Account forms.
fn provider_detail<'a>(app: &'a App, id: &str) -> iced::widget::Column<'a, Message> {
    let name = app
        .catalog
        .iter()
        .find(|(cid, _)| cid == id)
        .map(|(_, n)| n.clone())
        .unwrap_or_else(|| crate::model::provider_name(id));
    let entry = app.entries.iter().find(|e| e.provider == id);
    let meta = app.provider_meta.iter().find(|p| p.provider == id);
    let subline = match entry {
        Some(e) if !e.updated_at.trim().is_empty() => {
            let plan = crate::ui::popover::plan_display(&e.plan, id);
            let ago = updated_text(&e.updated_at);
            if plan.is_empty() { ago } else { format!("{plan} · {ago}") }
        }
        Some(_) => "Waiting for usage…".to_string(),
        None => "No usage data yet".to_string(),
    };
    let mut header = row![
        crate::icons::icon::<Message>(id, 26.0, Color::WHITE),
        column![
            text(name)
                .size(crate::ui::style::TEXT_TITLE2)
                .font(crate::ui::style::font_semibold()),
            text(subline)
                .size(crate::ui::style::TEXT_FOOTNOTE)
                .color(crate::ui::style::SECONDARY),
        ]
        .spacing(2)
        .width(Length::Fill),
        crate::ui::style::button(crate::ui::popover_charts::glyph(
            crate::ui::popover_charts::Glyph::Refresh,
            15.0,
            crate::ui::style::SECONDARY,
        ))
        .style(crate::ui::style::plain)
        .padding(4)
        .on_press_maybe((!app.busy).then_some(Message::Refresh)),
    ]
    .spacing(12)
    .align_y(iced::Alignment::Center);
    if let Some(m) = meta {
        let pid = id.to_string();
        header = header.push(
            iced::widget::toggler(m.enabled)
                .on_toggle(move |v| Message::SetProviderEnabled(pid.clone(), v))
                .size(22.0)
                .style(crate::ui::style::toggler),
        );
    }
    let mut info: Vec<Element<'a, Message>> = vec![container(header).padding([4, 0]).into()];
    let mut usage: Vec<Element<'a, Message>> = Vec::new();
    match entry {
        Some(e) => {
            if !e.error.trim().is_empty() {
                info.push(
                    text(e.error.clone())
                        .size(crate::ui::style::TEXT_CALLOUT)
                        .color(crate::ui::style::ERROR)
                        .into(),
                );
            }
            info.push(detail_row("Updated", format!("Updated {}", updated_text(&e.updated_at))));
            info.push(detail_row("Status", or_dash(&e.status).to_string()));
            info.push(detail_row("Account", or_dash(&e.account_label).to_string()));
            info.push(detail_row("Plan", or_dash(&crate::ui::popover::plan_display(&e.plan, id)).to_string()));
            let accent = crate::ui::popover::accent_rgb(id);
            let fill = Color::from_rgb8(accent.0, accent.1, accent.2);
            for w in &e.windows {
                usage.push(
                    column![
                        row![
                            text(format!("{} {}% left", w.label, w.remaining))
                                .size(crate::ui::style::TEXT_BODY)
                                .font(crate::ui::style::font_semibold())
                                .width(Length::Fill),
                            text(w.reset_text.clone())
                                .size(crate::ui::style::TEXT_FOOTNOTE)
                                .color(crate::ui::style::SECONDARY),
                        ]
                        .align_y(iced::Alignment::Center),
                        iced::widget::progress_bar(0.0..=100.0, w.remaining.clamp(0, 100) as f32)
                            .height(6.0)
                            .style(crate::ui::style::meter(fill, 6.0)),
                    ]
                    .spacing(6)
                    .into(),
                );
            }
            if let Some(c) = e.credits {
                usage.push(detail_row("Credits", format!("{c}")));
            }
            if !e.extra_usage_text.trim().is_empty() {
                usage.push(detail_row("Extra usage", e.extra_usage_text.clone()));
            }
        }
        None => info.push(note("No usage data for this provider yet — values show as — until a refresh returns data.")),
    }
    let mut col = column![section_form("", info, None)].spacing(20);
    if !usage.is_empty() {
        col = col.push(section_form("Usage", usage, None));
    }
    if let Some(auth) = meta.map(|m| &m.authentication).filter(|a| a.api_key || a.cookie_header) {
        let placeholder = if auth.api_key { "API key" } else { "Cookie header" };
        let has_draft = !app.credential_draft.trim().is_empty();
        let mut actions = row![].spacing(8);
        if auth.api_key {
            actions = actions.push(crate::ui::style::button(text("Save API key")).on_press_maybe(
                has_draft.then(|| Message::SaveCredential { provider: id.to_string(), cookie: false }),
            ));
        }
        if auth.cookie_header {
            actions = actions.push(crate::ui::style::button(text("Save cookie header")).on_press_maybe(
                has_draft.then(|| Message::SaveCredential { provider: id.to_string(), cookie: true }),
            ));
        }
        actions = actions.push(iced::widget::Space::with_width(Length::Fill));
        actions = actions.push(
            crate::ui::style::button(text("Clear stored credential"))
                .style(crate::ui::style::push)
                .on_press(Message::ClearCredential(id.to_string())),
        );
        let mut rows: Vec<Element<'a, Message>> = vec![
            crate::ui::style::text_input(placeholder, &app.credential_draft)
                .secure(true)
                .on_input(Message::CredentialDraftChanged)
                .into(),
            actions.into(),
        ];
        match &app.config_status {
            Ok(msg) if !msg.is_empty() => rows.push(note(msg)),
            Err(e) => rows.push(
                text(e.clone())
                    .size(crate::ui::style::TEXT_FOOTNOTE)
                    .color(crate::ui::style::ERROR)
                    .into(),
            ),
            _ => {}
        }
        col = col.push(section_form(
            "Credentials",
            rows,
            Some("Stored in the shared CodexBar config via the CLI (sent over stdin, never on the command line)."),
        ));
    }
    let mut account: Vec<Element<'a, Message>> = Vec::new();
    if id == "codex" || id == "claude" {
        let pid = id.to_string();
        account.push(pref(
            "Sign-in",
            row![
                crate::ui::style::button(text("Sign in…")).on_press(Message::AccountAction {
                    provider: pid.clone(),
                    login: true
                }),
                crate::ui::style::button(text("Sign out…")).on_press(Message::AccountAction {
                    provider: pid,
                    login: false
                }),
            ]
            .spacing(8)
            .into(),
        ));
    }
    if let Some(url) = dashboard_url(id) {
        account.push(link_row("Usage dashboard", url));
    }
    if let Some(url) = crate::ui::popover::status_url(id) {
        account.push(link_row("Status page", url));
    }
    if !account.is_empty() {
        col = col.push(section_form(
            "Account",
            account,
            Some("Sign-in opens your provider CLI in a terminal. Browser-cookie import works only on macOS; on Linux use the CLI sign-in or an API key."),
        ));
    }
    col
}

/// The full Usage & Spend dashboard (it draws its own header), then the cost toggle.
fn spend_pane(app: &App) -> Element<'_, Message> {
    // The dashboard carries its own "Track costs" toggle; Save persists it like every pane.
    column![
        crate::ui::spend_view::dashboard(app),
        row![
            container(note("Closing windows keeps CodexBar running.")).width(Length::Fill),
            crate::ui::style::button(text("Save")).on_press(Message::SaveSettings),
        ]
        .align_y(iced::Alignment::Center),
    ]
    .spacing(20)
    .into()
}

fn menu_pane(app: &App) -> Element<'_, Message> {
    let s = &app.settings;
    column![
        section_form(
            "Usage",
            vec![
                pref(
                    "Quota display",
                    settings_popup(
                        &[("Remaining", "remaining"), ("Used", "used")],
                        &s.quota_display,
                        s.clone(),
                        |st, v| st.quota_display = v.to_string(),
                    ),
                ),
                pref(
                    "Reset time display",
                    settings_popup(
                        &[("Countdown", "countdown"), ("Absolute time", "absolute"), ("Both", "both")],
                        &s.reset_display,
                        s.clone(),
                        |st, v| st.reset_display = v.to_string(),
                    ),
                ),
                check("Show pace", s.show_pace, s.clone(), |st, v| st.show_pace = v),
            ],
            None,
        ),
        section_form(
            "System",
            vec![column![
                extra_check(
                    crate::stay_awake::LABEL,
                    app.extras.stay_awake,
                    app.extras.clone(),
                    |e, v| e.stay_awake = v
                ),
                note(crate::stay_awake::SUBTITLE),
                note(app.stay_awake.status()),
            ]
            .spacing(3)
            .into()],
            Some("Saved immediately to ~/.config/codexbar/linux-extras.json (linux.json is untouched)."),
        ),
    ]
    .spacing(20)
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
        .style(crate::ui::style::slider)
    };
    column![
        crate::ui::menubar_editor::view(app),
        section_form(
            "Tray / bar",
            vec![
                pref(
                    "Tray style",
                    settings_popup(
                        &[("Meters", "meters"), ("Icon", "icon")],
                        &s.tray_style,
                        s.clone(),
                        |st, v| st.tray_style = v.to_string(),
                    ),
                ),
                check("Tray icon", s.show_tray, s.clone(), |st, v| st.show_tray = v),
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
            ],
            None,
        ),
    ]
    .spacing(20)
    .into()
}

fn extra_check<'a>(
    label: &str,
    cur: bool,
    base: crate::extras::Extras,
    set: impl Fn(&mut crate::extras::Extras, bool) + 'a,
) -> Element<'a, Message> {
    switch_row(label, cur, move |v| {
        let mut e = base.clone();
        set(&mut e, v);
        Message::ExtrasChanged(Box::new(e))
    })
}

fn notifications_pane(app: &App) -> Element<'_, Message> {
    let s = &app.settings;
    let x = &app.extras;
    let threshold_slider = {
        let base = s.clone();
        slider(1..=99, s.notify_threshold, move |v| {
            let mut st = base.clone();
            st.notify_threshold = v;
            Message::SettingsChanged(Box::new(st))
        })
        .step(1u32)
        .style(crate::ui::style::slider)
    };
    column![
        section_form(
            "Alerts",
            vec![
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
            ],
            Some("Delivered through the desktop notification service (org.freedesktop.Notifications)."),
        ),
        section_form(
            "On-screen effects",
            vec![
                extra_check(
                    "Show an on-screen alert when a quota warning fires",
                    x.quota_warning_overlay,
                    x.clone(),
                    |e, v| e.quota_warning_overlay = v
                ),
                extra_check(
                    "Confetti when the session limit resets",
                    x.confetti_on_session_reset,
                    x.clone(),
                    |e, v| e.confetti_on_session_reset = v
                ),
                extra_check(
                    "Confetti when the weekly limit resets",
                    x.confetti_on_weekly_reset,
                    x.clone(),
                    |e, v| e.confetti_on_weekly_reset = v
                ),
            ],
            Some("Click an effect to dismiss it; it also closes by itself after a few seconds. The alert needs the notifications option above. Compositors that ignore always-on-top or transparency show a plain window instead. Saved immediately to ~/.config/codexbar/linux-extras.json (linux.json is untouched)."),
        ),
    ]
    .spacing(20)
    .into()
}

fn hooks_pane(app: &App) -> Element<'_, Message> {
    let warning = row![
        crate::ui::popover_charts::glyph(
            crate::ui::popover_charts::Glyph::Warning,
            14.0,
            crate::ui::style::SECONDARY,
        ),
        note("Hooks can execute local commands on this machine. Only configure commands you trust."),
    ]
    .spacing(10)
    .align_y(iced::Alignment::Center);
    let (enabled, rules, error) = match &app.hooks {
        Some(Ok(h)) => (h.enabled, h.events.as_slice(), None),
        Some(Err(e)) => (false, &[][..], Some(e.as_str())),
        None => (false, &[][..], None),
    };
    let toggle = switch_row("Enable hooks", enabled, Message::SetHooksEnabled);
    let mut rule_rows: Vec<Element<'_, Message>> = Vec::new();
    match (&app.hooks, error) {
        (None, _) => rule_rows.push(note("Loading hooks…")),
        (_, Some(e)) => rule_rows.push(
            text(format!("Could not read hooks: {e}"))
                .size(crate::ui::style::TEXT_FOOTNOTE)
                .color(crate::ui::style::ERROR)
                .into(),
        ),
        _ if rules.is_empty() => rule_rows.push(note("No hooks configured.")),
        _ => {
            for r in rules {
                let provider = r.provider.clone().unwrap_or_else(|| "any provider".into());
                let command = std::iter::once(r.executable.as_str())
                    .chain(r.arguments.iter().map(String::as_str))
                    .collect::<Vec<_>>()
                    .join(" ");
                rule_rows.push(
                    row![
                        column![
                            text(format!("{} · {provider}", r.event)).size(crate::ui::style::TEXT_BODY),
                            text(command)
                                .size(crate::ui::style::TEXT_FOOTNOTE)
                                .color(crate::ui::style::SECONDARY),
                        ]
                        .spacing(2)
                        .width(Length::Fill),
                        text(if r.enabled { "On" } else { "Off" })
                            .size(crate::ui::style::TEXT_BODY)
                            .color(crate::ui::style::SECONDARY),
                    ]
                    .spacing(12)
                    .align_y(iced::Alignment::Center)
                    .into(),
                );
            }
        }
    }
    let cfg = config_json_path();
    let dir = cfg.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    rule_rows.push(
        row![
            crate::ui::style::button(text("Open config folder"))
                .on_press(Message::OpenFolder(dir.display().to_string())),
            crate::ui::style::button(text("Reload")).on_press(Message::RefreshHooks),
        ]
        .spacing(8)
        .into(),
    );
    column![
        section_form(
            "Hooks",
            vec![
                with_note(toggle, "Run external commands when quota or provider events occur."),
                warning.into(),
            ],
            None,
        ),
        section_form(
            "Rules",
            rule_rows,
            Some("Rules live in the shared config.json. Add and test them with `codexbar hooks list | test | watch`."),
        ),
    ]
    .spacing(20)
    .into()
}

fn plugins_pane(_app: &App) -> Element<'_, Message> {
    let dir = plugins_dir();
    let plugins = list_plugins();
    let dir_row = row![
        column![
            text("Plugins directory").size(crate::ui::style::TEXT_BODY),
            note_owned(tilde(&dir)),
        ]
        .spacing(2)
        .width(Length::Fill),
        crate::ui::style::button(text("Show in Files"))
            .on_press(Message::OpenFolder(dir.display().to_string())),
    ]
    .spacing(12)
    .align_y(iced::Alignment::Center);
    let installed: Element<'_, Message> = if plugins.is_empty() {
        container(
            column![
                crate::ui::popover_charts::glyph(
                    crate::ui::popover_charts::Glyph::Puzzle,
                    44.0,
                    crate::ui::style::SECONDARY,
                ),
                text("No Plugins Installed")
                    .size(crate::ui::style::TEXT_TITLE3)
                    .font(crate::ui::style::font_semibold())
                    .color(crate::ui::style::SECONDARY),
                text("Drop a .js or .ts file into the plugins directory.")
                    .size(crate::ui::style::TEXT_BODY)
                    .color(crate::ui::style::SECONDARY)
                    .align_x(iced::alignment::Horizontal::Center),
            ]
            .spacing(10)
            .align_x(iced::Alignment::Center),
        )
        .center_x(Length::Fill)
        .padding([40, 16])
        .style(crate::ui::style::panel)
        .into()
    } else {
        section_form(
            "Installed",
            plugins
                .into_iter()
                .map(|n| text(n).size(crate::ui::style::TEXT_BODY).into())
                .collect(),
            None,
        )
    };
    column![
        section_form(
            "Provider Plugins",
            vec![dir_row.into()],
            Some("Plugins are local JavaScript or TypeScript files. Network and cookie access require approval."),
        ),
        container(installed).padding([0, 4]),
    ]
    .spacing(20)
    .into()
}

fn advanced_pane(app: &App) -> Element<'_, Message> {
    let _ = app;
    column![
        section_form(
            "HTTP server / dashboard",
            vec![pref(
                "Usage summary",
                crate::ui::style::button(text("Copy usage summary"))
                    .on_press(Message::CopySummary)
                    .into(),
            )],
            Some("The CLI provides the web surfaces: `codexbar serve` runs the dashboard server (Bearer token required off-loopback) and `codexbar dashboard` prints a one-shot JSON snapshot. This desktop app does not embed its own HTTP server."),
        ),
        section_form(
            "Desktop IPC",
            vec![pref(
                "Socket",
                text(crate::ipc::socket_path().display().to_string())
                    .size(crate::ui::style::TEXT_BODY)
                    .color(crate::ui::style::SECONDARY)
                    .into(),
            )],
            Some("Other CodexBar surfaces (tray, Omarchy bar, `codexbar-linux --snapshot`) talk to the running app over this socket."),
        ),
    ]
    .spacing(20)
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
    let errors_text = if app.entries.is_empty() { "—".to_string() } else { errors.to_string() };
    column![
        section_form(
            "Usage",
            vec![
                detail_row("Usage entries loaded", app.entries.len().to_string()),
                detail_row("Entries with errors", errors_text),
                detail_row("Catalog providers", app.catalog.len().to_string()),
                detail_row("Last updated", or_dash(&app.updated).to_string()),
                detail_row(
                    "State",
                    format!("Busy: {} · Cost busy: {} · Stale: {}", app.busy, app.cost_busy, app.stale),
                ),
            ],
            None,
        ),
        section_form(
            "Errors",
            vec![
                with_note(detail_row("Last error", String::new()), or_dash(&app.error)),
                with_note(detail_row("Cost error", String::new()), or_dash(&app.cost_error)),
            ],
            None,
        ),
        section_form(
            "Paths",
            vec![
                with_note(detail_row("CLI executable", String::new()), or_dash(&app.settings.executable)),
                with_note(
                    pref("Settings file", container(crate::ui::style::text_input("", settings_path)).width(Length::Fixed(340.0)).into()),
                    "Select the field to copy the path.",
                ),
            ],
            Some("Run `codexbar diagnose --format json` for a full redacted diagnostic export."),
        ),
    ]
    .spacing(20)
    .into()
}

/// Plain full-width link row with a trailing arrow (Mac About "Links" rows).
fn link_row<'a>(label: &str, url: &str) -> Element<'a, Message> {
    crate::ui::style::button(
        row![
            text(label.to_string())
                .size(crate::ui::style::TEXT_BODY)
                .width(Length::Fill),
            crate::ui::popover_charts::glyph(
                crate::ui::popover_charts::Glyph::External,
                12.0,
                crate::ui::style::SECONDARY,
            ),
        ]
        .align_y(iced::Alignment::Center),
    )
    .on_press(Message::OpenUrl(url.to_string()))
    .style(crate::ui::style::plain)
    .padding(0)
    .width(Length::Fill)
    .into()
}

fn about_pane(_app: &App) -> Element<'_, Message> {
    let version = format!("Version {}", env!("CARGO_PKG_VERSION"));
    let icon = iced::widget::image(iced::widget::image::Handle::from_bytes(
        &include_bytes!("../../assets/app-icon.png")[..],
    ))
    .width(Length::Fixed(64.0))
    .height(Length::Fixed(64.0));
    // Mac About: centered hero card (icon, name, version, tagline), then Updates and Links.
    let hero = container(
        column![
            icon,
            text("CodexBar Extra")
                .size(crate::ui::style::TEXT_TITLE3)
                .font(crate::ui::style::font_semibold()),
            text(version.clone())
                .size(crate::ui::style::TEXT_CALLOUT)
                .color(crate::ui::style::SECONDARY),
            note("Usage and spend for Codex, Claude, and other AI coding providers — Linux desktop build."),
        ]
        .spacing(4)
        .align_x(iced::Alignment::Center),
    )
    .padding(20)
    .center_x(Length::Fill)
    .style(crate::ui::style::panel);
    column![
        container(hero).padding([0, 4]),
        section_form(
            "Updates",
            vec![
                text(version).size(crate::ui::style::TEXT_BODY).into(),
                with_note(
                    text("Updates unavailable in this build.")
                        .size(crate::ui::style::TEXT_BODY)
                        .color(crate::ui::style::SECONDARY)
                        .into(),
                    "Update through your package source or by rebuilding from the fork.",
                ),
            ],
            None,
        ),
        section_form(
            "Links",
            vec![
                link_row("GitHub — this fork", "https://github.com/FisiFla/codexbar-extra"),
                link_row("GitHub — upstream", "https://github.com/steipete/CodexBar"),
                link_row("Website", "https://codexbar.app"),
            ],
            None,
        ),
        container(note("© 2026 Peter Steinberger. MIT License.")).center_x(Length::Fill),
    ]
    .spacing(20)
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
            vec![SettingsPane::MenuBar, SettingsPane::Menu]
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
