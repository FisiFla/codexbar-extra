mod autostart;
mod cli;
mod deepseek;
mod descriptors;
mod ipc;
mod ipc_snapshot;
mod model;
mod notify;
mod settings;
mod shots;
mod spend;
mod state;
mod tray;
mod ui;

use state::{App, IpcCommand, Message, Page, SettingsPane, Shared, TrayCommand};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn main() -> iced::Result {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("codexbar-linux {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("CodexBar desktop for Linux\nUsage: codexbar-linux [--usage|--quick-view|--settings|--spending|--background|--snapshot[--with-spending]|--refresh|--quit|--no-tray|--cli PATH|--autostart status|enable|disable|--configure JSON]");
        return Ok(());
    }
    // Client forwarding: if a backend is running, send IPC and exit (Qt-compatible).
    let cmd = ipc::command_from_args(&args);
    let client_only = matches!(
        cmd.get("command").and_then(|v| v.as_str()),
        Some("snapshot" | "refresh" | "quit" | "configure" | "autostart")
    );
    if let Some(reply) = ipc::try_request(&cmd) {
        print!("{reply}");
        return Ok(());
    }
    if client_only {
        eprintln!("CodexBar desktop is not running");
        std::process::exit(1);
    }

    let initial = match cmd.get("command").and_then(|v| v.as_str()) {
        Some("quick-view") => Some(Page::QuickView),
        Some("settings") => Some(Page::Settings),
        Some("background") => None,
        _ => Some(Page::Usage),
    };
    let no_tray = args.iter().any(|a| a == "--no-tray");
    let cli_override = args
        .windows(2)
        .find(|w| w[0] == "--cli")
        .map(|w| w[1].clone());

    iced::daemon("CodexBar", update, view)
        .subscription(subscription)
        .theme(|_, _| iced::Theme::Dark)
        .run_with(move || {
            let mut settings = settings::load();
            if let Some(c) = cli_override.clone() {
                settings.executable = c;
            }
            if no_tray {
                settings.show_tray = false;
            }
            let shared = SHARED.with(|s| s.clone());
            *shared.lock().unwrap() = Shared {
                settings: settings.clone(),
                ..Default::default()
            };
            let app = App {
                settings,
                entries: vec![],
                prev_entries: vec![],
                spending: vec![],
                spend: Default::default(),
                catalog: vec![],
                provider_meta: vec![],
                credential_draft: String::new(),
                config_status: Ok(String::new()),
                busy: false,
                cost_busy: false,
                stale: false,
                error: String::new(),
                cost_error: String::new(),
                updated: String::new(),
                windows: BTreeMap::new(),
                selected: 0,
                settings_pane: SettingsPane::General,
                settings_search: String::new(),
                selected_provider: None,
                spend_period_days: 30,
                spend_tab: "providers".into(),
                pending_page: initial,
                cost_updated: None,
                cost_requested: None,
                show_about: false,
            };
            let cmds = vec![fetch_usage(&app), fetch_catalog(&app)];
            (app, iced::Task::batch(cmds))
        })
}

thread_local! {
    static SHARED: Arc<Mutex<Shared>> = Arc::new(Mutex::new(Shared::default()));
}

fn shared() -> Arc<Mutex<Shared>> {
    SHARED.with(|s| s.clone())
}

fn sync_shared(app: &App) {
    let mut s = shared().lock().unwrap().clone();
    s.entries = app.entries.clone();
    s.spending = app.spending.clone();
    s.busy = app.busy;
    s.cost_busy = app.cost_busy;
    s.stale = app.stale;
    s.error = app.error.clone();
    s.cost_error = app.cost_error.clone();
    s.updated = app.updated.clone();
    s.settings = app.settings.clone();
    s.quota_used = app.settings.quota_display == "used";
    s.summary = model::summary(&app.entries, s.quota_used);
    *shared().lock().unwrap() = s;
}

fn fetch_usage(app: &App) -> iced::Task<Message> {
    let (exe, provider, source, show_status, show_identity) = (
        app.settings.executable.clone(),
        app.settings.provider.clone(),
        app.settings.source.clone(),
        app.settings.show_status,
        app.settings.show_identity,
    );
    iced::Task::perform(
        async move { cli::fetch_entries(&exe, &provider, &source, show_status, show_identity).await },
        Message::UsageLoaded,
    )
}

fn fetch_costs(app: &App) -> iced::Task<Message> {
    let exe = app.settings.executable.clone();
    iced::Task::perform(
        async move { cli::fetch_costs(&exe).await },
        Message::CostLoaded,
    )
}

fn fetch_catalog(app: &App) -> iced::Task<Message> {
    let exe = app.settings.executable.clone();
    iced::Task::perform(
        async move { cli::fetch_catalog(&exe).await },
        Message::CatalogLoaded,
    )
}

fn open_page(app: &mut App, page: Page) -> iced::Task<Message> {
    if let Some((id, _)) = app.windows.iter().find(|(_, p)| **p == page) {
        let id = *id;
        return iced::window::gain_focus(id);
    }
    let (size, title) = match page {
        Page::QuickView => ((420.0, 680.0), "CodexBar"),
        Page::Usage => ((880.0, 620.0), "CodexBar — Usage & Spend"),
        Page::Settings => ((880.0, 620.0), "CodexBar Settings"),
    };
    let (id, task) = iced::window::open(iced::window::Settings {
        size: iced::Size::new(size.0, size.1),
        ..Default::default()
    });
    app.windows.insert(id, page);
    let _ = title;
    let mut tasks = vec![task.map(move |_| Message::WindowOpened(id, page))];
    if app.settings.refresh_on_open {
        tasks.push(fetch_usage(app));
    }
    if page == Page::Usage {
        tasks.push(fetch_costs(app));
    }
    iced::Task::batch(tasks)
}

fn update(app: &mut App, msg: Message) -> iced::Task<Message> {
    match msg {
        Message::None => iced::Task::none(),
        Message::Tick => {
            if let Some(p) = app.pending_page.take() {
                return open_page(app, p);
            }
            shots::tick(app).unwrap_or_else(iced::Task::none)
        }
        Message::Refresh => {
            app.busy = true;
            sync_shared(app);
            fetch_usage(app)
        }
        Message::RefreshCosts => {
            app.cost_busy = true;
            sync_shared(app);
            fetch_costs(app)
        }
        Message::UsageLoaded(Ok(entries)) => {
            if app.settings.notifications {
                notify::maybe_notify(&app.prev_entries, &entries, app.settings.notify_threshold);
            }
            app.prev_entries = std::mem::replace(&mut app.entries, entries);
            app.busy = false;
            app.stale = false;
            app.error.clear();
            app.updated = chrono::Local::now().format("%H:%M").to_string();
            if app.selected as usize >= app.entries.len() && !app.entries.is_empty() {
                app.selected = 0;
            }
            sync_shared(app);
            iced::Task::none()
        }
        Message::UsageLoaded(Err(e)) => {
            app.busy = false;
            app.stale = true;
            app.error = e;
            sync_shared(app);
            iced::Task::none()
        }
        Message::CostLoaded(Ok(costs)) => {
            app.spending = costs.clone();
            app.spend = spend::build_from_entries(&costs);
            app.cost_busy = false;
            app.cost_error.clear();
            app.cost_updated = Some(std::time::Instant::now());
            sync_shared(app);
            iced::Task::none()
        }
        Message::CostLoaded(Err(e)) => {
            app.cost_busy = false;
            app.cost_error = e;
            sync_shared(app);
            iced::Task::none()
        }
        Message::CatalogLoaded(c) => {
            app.catalog = c
                .iter()
                .map(|p| (p.provider.clone(), p.display_name.clone()))
                .collect();
            descriptors::register_catalog(&app.catalog);
            app.provider_meta = c;
            iced::Task::none()
        }
        Message::ShowPage(p) => open_page(app, p),
        Message::WindowOpened(_, _) => iced::Task::none(),
        Message::WindowClosed(id) => {
            app.windows.remove(&id);
            iced::Task::none()
        }
        Message::SelectEntry(i) => {
            app.selected = i;
            iced::Task::none()
        }
        Message::TrayCmd(c) => match c {
            TrayCommand::QuickView => open_page(app, Page::QuickView),
            TrayCommand::Usage => open_page(app, Page::Usage),
            TrayCommand::Settings => open_page(app, Page::Settings),
            TrayCommand::Refresh => {
                app.busy = true;
                iced::Task::batch([fetch_usage(app), fetch_costs(app)])
            }
            TrayCommand::Quit => iced::exit(),
        },
        Message::IpcCmd(c) => match c {
            IpcCommand::Show(p) => open_page(
                app,
                match p.as_str() {
                    "quick-view" => Page::QuickView,
                    "settings" => Page::Settings,
                    _ => Page::Usage,
                },
            ),
            IpcCommand::Refresh => {
                app.busy = true;
                fetch_usage(app)
            }
            IpcCommand::RefreshCostsIfStale => {
                let older = |t: Option<std::time::Instant>, secs| {
                    t.is_none_or(|t| t.elapsed() > Duration::from_secs(secs))
                };
                if !app.cost_busy
                    && older(app.cost_updated, 300)
                    && (app.cost_error.is_empty() || older(app.cost_requested, 5))
                {
                    app.cost_requested = Some(std::time::Instant::now());
                    app.cost_busy = true;
                    sync_shared(app);
                    return fetch_costs(app);
                }
                iced::Task::none()
            }
            IpcCommand::Quit => iced::exit(),
            IpcCommand::ReloadSettings => {
                app.settings = settings::load();
                sync_shared(app);
                iced::Task::none()
            }
        },
        Message::OpenSettingsPane(p) => {
            app.settings_pane = p;
            app.selected_provider = None;
            iced::Task::none()
        }
        Message::SettingsSearchChanged(s) => {
            app.settings_search = s;
            iced::Task::none()
        }
        Message::SelectProvider(p) => {
            app.selected_provider = Some(p);
            app.settings_pane = SettingsPane::Providers;
            iced::Task::none()
        }
        Message::SettingsChanged(s) => {
            app.settings = *s;
            sync_shared(app);
            iced::Task::none()
        }
        Message::SaveSettings => {
            match settings::save(&app.settings) {
                Ok(()) => app.error.clear(),
                Err(e) => app.error = e,
            }
            sync_shared(app);
            fetch_usage(app)
        }
        Message::SetAutostart(enabled) => iced::Task::perform(
            async move { autostart::set(enabled).map(|_| enabled) },
            Message::AutostartResult,
        ),
        Message::AutostartResult(_) => iced::Task::none(),
        Message::SetSpendPeriod(d) => {
            app.spend_period_days = d;
            fetch_costs(app)
        }
        Message::SetSpendTab(t) => {
            app.spend_tab = t;
            iced::Task::none()
        }
        Message::AccountAction { provider, login } => iced::Task::perform(
            async move {
                cli::account_action(&provider, login).await;
            },
            |_| Message::Refresh,
        ),
        Message::SetProviderEnabled(provider, enabled) => {
            let exe = app.settings.executable.clone();
            iced::Task::perform(
                async move {
                    cli::config_set_enabled(&exe, &provider, enabled)
                        .await
                        .map(|_| {
                            format!(
                                "{provider} {}",
                                if enabled { "enabled" } else { "disabled" }
                            )
                        })
                },
                Message::ConfigDone,
            )
        }
        Message::CredentialDraftChanged(v) => {
            app.credential_draft = v;
            iced::Task::none()
        }
        Message::SaveCredential { provider, cookie } => {
            let exe = app.settings.executable.clone();
            let secret = std::mem::take(&mut app.credential_draft);
            iced::Task::perform(
                async move {
                    let res = if cookie {
                        cli::config_set_cookie(&exe, &provider, secret.trim()).await
                    } else {
                        cli::config_set_api_key(&exe, &provider, secret.trim()).await
                    };
                    res.map(|_| {
                        format!(
                            "Saved {} for {provider}",
                            if cookie { "cookie header" } else { "API key" }
                        )
                    })
                },
                Message::ConfigDone,
            )
        }
        Message::ClearCredential(provider) => {
            app.credential_draft.clear();
            let exe = app.settings.executable.clone();
            iced::Task::perform(
                async move {
                    cli::config_clear_credential(&exe, &provider)
                        .await
                        .map(|_| format!("Cleared stored credentials for {provider}"))
                },
                Message::ConfigDone,
            )
        }
        Message::ConfigDone(result) => {
            let ok = result.is_ok();
            app.config_status = result;
            if ok {
                app.busy = true;
                return iced::Task::batch([fetch_catalog(app), fetch_usage(app)]);
            }
            iced::Task::none()
        }
        Message::OpenUrl(u) => {
            let _ = std::process::Command::new("xdg-open").arg(u).spawn();
            iced::Task::none()
        }
        Message::CopySummary => iced::clipboard::write(model::summary(
            &app.entries,
            app.settings.quota_display == "used",
        )),
        Message::Quit => iced::exit(),
    }
}

fn view(app: &App, id: iced::window::Id) -> iced::Element<Message> {
    match app.windows.get(&id) {
        Some(Page::QuickView) => ui::popover::view(app),
        Some(Page::Usage) => ui::spend_view::view(app),
        Some(Page::Settings) => ui::settings_view::view(app),
        None => iced::widget::text("CodexBar").into(),
    }
}

fn subscription(app: &App) -> iced::Subscription<Message> {
    let tick = iced::time::every(Duration::from_secs(1)).map(|_| Message::Tick);
    let poll = iced::time::every(Duration::from_secs(
        app.settings.refresh_seconds.max(60) as u64
    ))
    .map(|_| Message::Refresh);
    let closed = iced::window::close_events().map(Message::WindowClosed);
    let tray_sub = iced::Subscription::run(|| tray::stream(shared())).map(Message::TrayCmd);
    let ipc_sub = iced::Subscription::run(|| ipc::stream(shared())).map(Message::IpcCmd);
    iced::Subscription::batch([tick, poll, closed, tray_sub, ipc_sub])
}

#[cfg(test)]
mod tests {
    #[test]
    fn fixtures_parse() {
        let rows =
            crate::cli::parse_usage(include_bytes!("../tests/fixtures/usage-codex.json")).unwrap();
        assert_eq!(rows[0].provider, "codex");
        assert_eq!(
            rows[0]
                .usage
                .as_ref()
                .unwrap()
                .primary
                .as_ref()
                .unwrap()
                .remaining_percent(),
            99
        );
        crate::cli::parse_cost(include_bytes!("../tests/fixtures/cost-both.json")).unwrap();
        crate::cli::parse_providers(include_bytes!("../tests/fixtures/providers-sample.json"))
            .unwrap();
    }
    #[test]
    fn qt_defaults() {
        let s = crate::settings::Settings::default();
        assert_eq!(s.refresh_seconds, 300);
        s.validate().unwrap();
    }
}
