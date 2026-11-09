mod autostart;
mod charts;
mod cli;
mod deepseek;
mod descriptors;
mod extras;
mod focus;
mod history;
mod ipc;
mod ipc_snapshot;
mod menubar_layout;
mod model;
mod notify;
mod openai_live;
mod overlay;
mod scan;
mod sessions;
mod settings;
mod share;
mod shots;
mod spend;
mod state;
mod stay_awake;
mod storage;
mod throughput;
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
        println!("CodexBar desktop for Linux\nUsage: codexbar-linux [--usage|--quick-view|--settings|--spending|--sessions|--background|--snapshot[--with-spending]|--refresh|--quit|--no-tray|--cli PATH|--autostart status|enable|disable|--configure JSON]");
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
        Some("sessions") => Some(Page::Sessions),
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
        .theme(|app, id| match app.windows.get(&id) {
            Some(Page::QuotaAlert | Page::Confetti) => ui::overlay_view::clear_theme(),
            _ => ui::style::mac_theme(),
        })
        .run_with(move || {
            let mut settings = settings::load();
            if let Some(c) = cli_override.clone() {
                settings.executable = c;
            }
            if no_tray {
                settings.show_tray = false;
            }
            let menubar_layout = menubar_layout::load();
            let shared = SHARED.with(|s| s.clone());
            *shared.lock().unwrap() = Shared {
                settings: settings.clone(),
                menubar_layout: menubar_layout.clone(),
                ..Default::default()
            };
            let history_path = history::state_path();
            let history = history_path
                .as_deref()
                .map(history::load)
                .unwrap_or_default();
            let app = App {
                settings,
                menubar_layout,
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
                sessions: vec![],
                sessions_error: String::new(),
                sessions_busy: false,
                sessions_loaded: false,
                cost_payloads: vec![],
                expanded_charts: Default::default(),
                chart_hover: None,
                chart_series: Default::default(),
                history,
                history_path,
                storage: Default::default(),
                storage_loaded: false,
                share_dark: true,
                share_status: String::new(),
                live: Default::default(),
                scan: None,
                scan_view: Default::default(),
                scan_busy: false,
                scan_loaded: false,
                scan_zone: scan::aggregate::Zone::Local,
                session_query: String::new(),
                session_sort: state::SessionSort::Recent,
                selected_session: None,
                tool_filter: String::new(),
                tools_expanded: false,
                extras: extras::load(),
                alert: None,
                confetti: None,
                focus_status: String::new(),
                stay_awake: stay_awake::StayAwake::new(),
            };
            let cmds = vec![fetch_usage(&app), fetch_catalog(&app), scan_storage()];
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
    s.menubar_layout = app.menubar_layout.clone();
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

/// Directory walk on a blocking thread so the UI never waits on it.
fn scan_storage() -> iced::Task<Message> {
    iced::Task::perform(
        async { tokio::task::spawn_blocking(storage::scan_all).await },
        |r| Message::StorageScanned(r.unwrap_or_default()),
    )
}

fn save_history(app: &App) -> iced::Task<Message> {
    let Some(path) = app.history_path.clone() else {
        return iced::Task::none();
    };
    let history = app.history.clone();
    iced::Task::perform(
        async move {
            tokio::task::spawn_blocking(move || history::save(&path, &history))
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r.map_err(|e| e.to_string()))
        },
        Message::HistorySaved,
    )
}

/// Session/tool scan on a blocking worker; the UI keeps rendering the previous aggregate.
fn fetch_scan(app: &mut App) -> iced::Task<Message> {
    if app.scan_busy || !app.settings.show_costs {
        return iced::Task::none();
    }
    app.scan_busy = true;
    iced::Task::perform(
        async { std::sync::Arc::new(scan::scan_default().await) },
        Message::ScanLoaded,
    )
}

/// Re-derive the session/tool view for the current period and time zone (pure, cheap).
fn reaggregate(app: &mut App) {
    if let Some(snap) = &app.scan {
        app.scan_view = scan::aggregate::aggregate(
            snap,
            scan::aggregate::Filter {
                since_ms: scan::period_since_ms(app.spend_period_days),
                zone: app.scan_zone,
            },
        );
    }
}

fn fetch_sessions(app: &mut App) -> iced::Task<Message> {
    app.sessions_busy = true;
    let exe = app.settings.executable.clone();
    iced::Task::perform(
        async move { cli::fetch_sessions(&exe).await },
        Message::SessionsLoaded,
    )
}

/// Hold or drop the idle inhibitor from the current setting and the last session list.
fn sync_stay_awake(app: &mut App) {
    app.stay_awake.sync(
        app.extras.stay_awake,
        stay_awake::has_live_session(&app.sessions),
    );
}

fn fetch_catalog(app: &App) -> iced::Task<Message> {
    let exe = app.settings.executable.clone();
    iced::Task::perform(
        async move { cli::fetch_catalog(&exe).await },
        Message::CatalogLoaded,
    )
}

/// Starts one Live Usage fetch (no-op while one is in flight). The key is resolved inside the
/// blocking task from the shared CodexBar config and never leaves it. The screenshot hook
/// never hits the network.
fn fetch_live(app: &mut App) -> iced::Task<Message> {
    if app.live.loading || std::env::var_os("CODEXBAR_SCREENSHOT_DIR").is_some() {
        return iced::Task::none();
    }
    app.live.loading = true;
    app.live.next_refresh = None;
    let (generation, window) = (app.live.generation, app.live.window_minutes);
    iced::Task::perform(
        async move {
            tokio::task::spawn_blocking(move || openai_live::fetch_live(window))
                .await
                .unwrap_or_else(|_| {
                    Err(openai_live::LiveError::Network("Live usage failed.".into()))
                })
        },
        move |r| Message::OpenAiLiveLoaded(generation, r),
    )
}

/// Opens a transient overlay window: undecorated, transparent, always on top, not in the way
/// of the pointer once dismissed. Compositors that ignore any of those hints still get a
/// plain, click-to-dismiss, auto-expiring window.
fn open_overlay(app: &mut App, page: Page, size: (f32, f32)) -> iced::Task<Message> {
    let stale: Vec<_> = app
        .windows
        .iter()
        .filter(|(_, p)| **p == page)
        .map(|(id, _)| *id)
        .collect();
    let mut tasks: Vec<iced::Task<Message>> = stale.into_iter().map(iced::window::close).collect();
    let (id, open) = iced::window::open(iced::window::Settings {
        size: iced::Size::new(size.0, size.1),
        position: iced::window::Position::Centered,
        decorations: false,
        transparent: true,
        resizable: false,
        level: iced::window::Level::AlwaysOnTop,
        ..Default::default()
    });
    app.windows.insert(id, page);
    tasks.push(open.map(move |_| Message::WindowOpened(id, page)));
    iced::Task::batch(tasks)
}

fn close_overlays(app: &mut App) -> iced::Task<Message> {
    app.alert = None;
    app.confetti = None;
    let ids: Vec<_> = app
        .windows
        .iter()
        .filter(|(_, p)| matches!(p, Page::QuotaAlert | Page::Confetti))
        .map(|(id, _)| *id)
        .collect();
    iced::Task::batch(ids.into_iter().map(iced::window::close))
}

fn show_alert(app: &mut App, alert: overlay::Alert) -> iced::Task<Message> {
    app.alert = Some(ui::overlay_view::AlertState {
        alert,
        started: std::time::Instant::now(),
    });
    open_overlay(app, Page::QuotaAlert, ui::overlay_view::ALERT_SIZE)
}

fn show_confetti(app: &mut App, provider: String) -> iced::Task<Message> {
    app.confetti = Some(ui::overlay_view::ConfettiState {
        provider,
        started: std::time::Instant::now(),
    });
    open_overlay(app, Page::Confetti, ui::overlay_view::CONFETTI_SIZE)
}

fn open_page(app: &mut App, page: Page) -> iced::Task<Message> {
    if matches!(page, Page::QuotaAlert | Page::Confetti) {
        return iced::Task::none();
    }
    if let Some((id, _)) = app.windows.iter().find(|(_, p)| **p == page) {
        let id = *id;
        return iced::window::gain_focus(id);
    }
    let (size, title) = match page {
        Page::QuickView => ((420.0, 680.0), "CodexBar"),
        Page::Usage => ((880.0, 620.0), "CodexBar — Usage & Spend"),
        Page::Settings => ((880.0, 620.0), "CodexBar Settings"),
        Page::Sessions => ((560.0, 680.0), "CodexBar — Agent Sessions"),
        Page::ShareStats => (ui::share_view::WINDOW_SIZE, "Share AI Usage"),
        Page::OpenAiLive => ((880.0, 640.0), "OpenAI Live Usage"),
        Page::QuotaAlert | Page::Confetti => unreachable!("overlays use open_overlay"),
    };
    let (id, task) = iced::window::open(iced::window::Settings {
        size: iced::Size::new(size.0, size.1),
        resizable: page != Page::ShareStats,
        ..Default::default()
    });
    app.windows.insert(id, page);
    let _ = title;
    let mut tasks = vec![task.map(move |_| Message::WindowOpened(id, page))];
    if page == Page::OpenAiLive {
        tasks.push(fetch_live(app));
    }
    if page == Page::Sessions {
        tasks.push(fetch_sessions(app));
    } else if app.settings.refresh_on_open {
        tasks.push(fetch_usage(app));
    }
    if page == Page::Usage
        || page == Page::ShareStats
        || (page == Page::QuickView && app.cost_updated.is_none())
    {
        tasks.push(fetch_costs(app));
    }
    if page == Page::Usage {
        tasks.push(fetch_scan(app));
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
            let live_due = app
                .live
                .next_refresh
                .is_some_and(|t| t <= std::time::Instant::now());
            if live_due && app.windows.values().any(|p| *p == Page::OpenAiLive) {
                return fetch_live(app);
            }
            if !overlay::forced() {
                let expired = |t: std::time::Instant, secs: f32| t.elapsed().as_secs_f32() > secs;
                let done = app
                    .alert
                    .as_ref()
                    .is_some_and(|a| expired(a.started, overlay::ALERT_SECS))
                    || app
                        .confetti
                        .as_ref()
                        .is_some_and(|c| expired(c.started, overlay::CONFETTI_SECS));
                if done {
                    let alert_done = app
                        .alert
                        .as_ref()
                        .is_some_and(|a| expired(a.started, overlay::ALERT_SECS));
                    let page = if alert_done {
                        Page::QuotaAlert
                    } else {
                        Page::Confetti
                    };
                    if alert_done {
                        app.alert = None;
                    } else {
                        app.confetti = None;
                    }
                    let ids: Vec<_> = app
                        .windows
                        .iter()
                        .filter(|(_, p)| **p == page)
                        .map(|(id, _)| *id)
                        .collect();
                    return iced::Task::batch(ids.into_iter().map(iced::window::close));
                }
            }
            shots::tick(app).unwrap_or_else(iced::Task::none)
        }
        Message::OverlayFrame => iced::Task::none(),
        Message::Redrawn(id) => {
            shots::on_redraw(id);
            iced::Task::none()
        }
        Message::DismissOverlays => close_overlays(app),
        Message::ForceOverlays => iced::Task::batch([
            show_alert(
                app,
                overlay::Alert {
                    title: "CodexBar · Codex".into(),
                    message: "Session: 8% quota remaining.".into(),
                },
            ),
            show_confetti(app, "codex".into()),
        ]),
        Message::ExtrasChanged(e) => {
            let was = app.extras.stay_awake;
            app.extras = *e;
            match extras::save(&app.extras) {
                Ok(()) => app.error.clear(),
                Err(e) => app.error = e,
            }
            sync_stay_awake(app);
            if app.extras.stay_awake && !was {
                return fetch_sessions(app);
            }
            iced::Task::none()
        }
        Message::FocusSession(pid) => {
            app.focus_status = "Focusing\u{2026}".into();
            iced::Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || focus::focus_session(pid))
                        .await
                        .unwrap_or_else(|e| Err(e.to_string()))
                },
                Message::FocusDone,
            )
        }
        Message::FocusDone(r) => {
            app.focus_status = r.unwrap_or_else(|e| e);
            iced::Task::none()
        }
        Message::Refresh => {
            app.busy = true;
            sync_shared(app);
            fetch_usage(app)
        }
        Message::RefreshCosts => {
            app.cost_busy = true;
            sync_shared(app);
            iced::Task::batch([fetch_costs(app), fetch_scan(app)])
        }
        Message::ScanLoaded(snap) => {
            app.scan = Some(snap);
            app.scan_busy = false;
            app.scan_loaded = true;
            reaggregate(app);
            iced::Task::none()
        }
        Message::SetScanZone(z) => {
            app.scan_zone = z;
            reaggregate(app);
            iced::Task::none()
        }
        Message::SessionQuery(q) => {
            app.session_query = q;
            iced::Task::none()
        }
        Message::SessionSort(s) => {
            app.session_sort = s;
            iced::Task::none()
        }
        Message::SelectSession(s) => {
            app.selected_session = s;
            app.tool_filter.clear();
            app.tools_expanded = false;
            iced::Task::none()
        }
        Message::ToolFilter(q) => {
            app.tool_filter = q;
            iced::Task::none()
        }
        Message::ToggleToolsExpanded => {
            app.tools_expanded = !app.tools_expanded;
            iced::Task::none()
        }
        Message::UsageLoaded(Ok(entries)) => {
            let wants_events = app.settings.notifications
                || app.extras.quota_warning_overlay
                || app.extras.confetti_on_session_reset
                || app.extras.confetti_on_weekly_reset;
            let events = if wants_events {
                notify::transition_events(
                    &app.prev_entries,
                    &entries,
                    app.settings.notify_threshold,
                )
            } else {
                Vec::new()
            };
            if app.settings.notifications {
                notify::post(&events);
            }
            let fired = overlay::triggers(&events, app.settings.notifications, &app.extras);
            let mut overlay_tasks = Vec::new();
            if let Some(a) = fired.alert {
                overlay_tasks.push(show_alert(app, a));
            }
            if let Some(c) = fired.confetti {
                overlay_tasks.push(show_confetti(app, c.provider));
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
            let obs = history::observations(&app.entries);
            let now = chrono::Utc::now().timestamp();
            let credit_obs = history::credit_observations(&app.entries);
            let mut history_changed = app.history.record(&obs, now);
            history_changed |= app.history.record_credits(&credit_obs, now);
            if history_changed {
                overlay_tasks.push(save_history(app));
            }
            iced::Task::batch(overlay_tasks)
        }
        Message::UsageLoaded(Err(e)) => {
            app.busy = false;
            app.stale = true;
            app.error = e;
            sync_shared(app);
            iced::Task::none()
        }
        Message::CostLoaded(Ok((costs, payloads))) => {
            app.spending = costs.clone();
            app.spend = spend::build_from_entries(&costs);
            // Entries carry no model breakdowns; take model ranking from the raw payloads.
            app.spend.by_model = spend::build(&payloads).by_model;
            app.cost_payloads = payloads;
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
        Message::ToggleChart(key) => {
            if !app.expanded_charts.remove(&key) {
                app.expanded_charts.insert(key);
            }
            app.chart_hover = None;
            iced::Task::none()
        }
        Message::ChartSeries { key, index } => {
            app.chart_series.insert(key, index);
            iced::Task::none()
        }
        Message::HistorySaved(r) => {
            if let Err(e) = r {
                eprintln!("usage history not saved: {e}");
            }
            iced::Task::none()
        }
        Message::ScanStorage => scan_storage(),
        Message::StorageScanned(map) => {
            app.storage = map;
            app.storage_loaded = true;
            iced::Task::none()
        }
        Message::ChartHover { key, index } => {
            match index {
                Some(i) => app.chart_hover = Some((key, i)),
                // Only the chart that owns the hover may clear it (cursor moving between charts).
                None if app.chart_hover.as_ref().is_some_and(|(k, _)| *k == key) => {
                    app.chart_hover = None
                }
                None => {}
            }
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
            // A replaced overlay closes after its successor is registered; only clear the
            // state when no window of that page is left.
            if let Some(page) = app.windows.remove(&id) {
                if !app.windows.values().any(|p| *p == page) {
                    match page {
                        Page::QuotaAlert => app.alert = None,
                        Page::Confetti => app.confetti = None,
                        Page::OpenAiLive => {
                            // Drop in-flight results and stop polling while the window is closed.
                            app.live.generation += 1;
                            app.live.loading = false;
                            app.live.next_refresh = None;
                        }
                        _ => {}
                    }
                }
            }
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
            TrayCommand::Sessions => open_page(app, Page::Sessions),
            TrayCommand::ShareStats => open_page(app, Page::ShareStats),
            TrayCommand::OpenAiLive => open_page(app, Page::OpenAiLive),
            TrayCommand::Refresh => {
                app.busy = true;
                iced::Task::batch([fetch_usage(app), fetch_costs(app)])
            }
            TrayCommand::Quit => {
                app.stay_awake.release();
                iced::exit()
            }
        },
        Message::IpcCmd(c) => match c {
            IpcCommand::Show(p) => open_page(
                app,
                match p.as_str() {
                    "quick-view" => Page::QuickView,
                    "settings" => Page::Settings,
                    "sessions" => Page::Sessions,
                    "share-stats" => Page::ShareStats,
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
            IpcCommand::Quit => {
                app.stay_awake.release();
                iced::exit()
            }
            IpcCommand::ReloadSettings => {
                app.settings = settings::load();
                app.extras = extras::load();
                sync_stay_awake(app);
                app.menubar_layout = menubar_layout::load();
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
        Message::LayoutEdit(edit) => {
            // Own file, saved on every edit so the tray follows immediately.
            menubar_layout::apply(&mut app.menubar_layout, edit);
            match menubar_layout::save(&app.menubar_layout) {
                Ok(()) => app.error.clear(),
                Err(e) => app.error = e,
            }
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
            reaggregate(app);
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
        Message::RefreshSessions => fetch_sessions(app),
        Message::SessionsLoaded(result) => {
            app.sessions_busy = false;
            app.sessions_loaded = true;
            match result {
                Ok(rows) => {
                    app.sessions = rows;
                    app.sessions_error.clear();
                    sync_stay_awake(app);
                }
                // Keep the last good list on screen; the window shows the error above it.
                Err(e) => app.sessions_error = e,
            }
            iced::Task::none()
        }
        Message::OpenFolder(dir) => {
            let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
            iced::Task::none()
        }
        Message::CopyText(t) => iced::clipboard::write(t),
        Message::OpenUrl(u) => {
            let _ = std::process::Command::new("xdg-open").arg(u).spawn();
            iced::Task::none()
        }
        Message::CopySummary => iced::clipboard::write(model::summary(
            &app.entries,
            app.settings.quota_display == "used",
        )),
        Message::ShareSetPeriod(d) => {
            app.spend_period_days = d;
            reaggregate(app);
            iced::Task::none()
        }
        Message::ShareSetDark(d) => {
            app.share_dark = d;
            iced::Task::none()
        }
        Message::ShareSave => share_capture(app, state::ShareAction::Save),
        Message::ShareCopyImage => share_capture(app, state::ShareAction::CopyImage),
        Message::ShareCopyText => match ui::share_view::current_payload(app) {
            Some(p) => {
                app.share_status = "Stats copied".into();
                iced::clipboard::write(share::text(&p))
            }
            None => iced::Task::none(),
        },
        Message::ShareCaptured(action, Err(e)) => {
            let _ = action;
            if std::env::var_os("CODEXBAR_SCREENSHOT_DIR").is_some() {
                eprintln!("share: {e}");
            }
            app.share_status = e;
            iced::Task::none()
        }
        Message::ShareCaptured(action, Ok(png)) => iced::Task::perform(
            async move {
                match action {
                    state::ShareAction::Save => {
                        share::save_png(&png).map(|p| format!("Image saved to {}", p.display()))
                    }
                    state::ShareAction::CopyImage => share::copy_png(png)
                        .await
                        .map(|_| "Image copied".to_string()),
                }
            },
            Message::ShareDone,
        ),
        Message::ShareDone(result) => {
            app.share_status = result.unwrap_or_else(|e| e);
            if std::env::var_os("CODEXBAR_SCREENSHOT_DIR").is_some() {
                eprintln!("share: {}", app.share_status);
            }
            iced::Task::none()
        }
        Message::OpenAiLiveRefresh => fetch_live(app),
        Message::OpenAiLiveRange(minutes) => {
            if app.live.window_minutes != minutes {
                app.live.window_minutes = minutes;
                app.live.generation += 1;
                app.live.loading = false;
                return fetch_live(app);
            }
            iced::Task::none()
        }
        Message::OpenAiLiveFilter(f) => {
            app.live.filter = f;
            app.chart_hover = None;
            iced::Task::none()
        }
        Message::OpenAiLiveOpenSettings => {
            app.selected_provider = Some("openai".into());
            app.settings_pane = SettingsPane::Providers;
            open_page(app, Page::Settings)
        }
        Message::OpenAiLiveLoaded(generation, result) => {
            if generation != app.live.generation {
                return iced::Task::none();
            }
            let live = &mut app.live;
            live.loading = false;
            match result {
                Ok(snapshot) => {
                    live.last_updated = Some(snapshot.fetched_at);
                    live.snapshot = Some(snapshot);
                    live.error = None;
                }
                // Keep the last good snapshot on screen; the window shows the error above it.
                Err(e) => live.error = Some(e),
            }
            live.interval_secs =
                openai_live::next_interval(live.error.as_ref(), live.interval_secs);
            live.next_refresh =
                Some(std::time::Instant::now() + Duration::from_secs(live.interval_secs));
            iced::Task::none()
        }
        Message::Quit => {
            app.stay_awake.release();
            iced::exit()
        }
    }
}

/// Screenshots the Share Stats window and crops it to the card; the result
/// comes back as `ShareCaptured` for saving or copying.
pub(crate) fn share_capture(app: &mut App, action: state::ShareAction) -> iced::Task<Message> {
    let Some((id, _)) = app.windows.iter().find(|(_, p)| **p == Page::ShareStats) else {
        return iced::Task::none();
    };
    app.share_status = "Rendering image…".into();
    iced::window::screenshot(*id).map(move |shot| {
        Message::ShareCaptured(
            action,
            share::crop_to_png(
                &shot.bytes,
                (shot.size.width, shot.size.height),
                shot.scale_factor,
                [
                    ui::share_view::PAD,
                    ui::share_view::PAD,
                    ui::share_view::CARD_W,
                    ui::share_view::CARD_H,
                ],
            ),
        )
    })
}

fn view(app: &App, id: iced::window::Id) -> iced::Element<Message> {
    match app.windows.get(&id) {
        Some(Page::QuickView) => ui::popover::view(app),
        Some(Page::Usage) => ui::spend_view::view(app),
        Some(Page::Settings) => ui::settings_view::view(app),
        Some(Page::Sessions) => ui::sessions_view::view(app),
        Some(Page::ShareStats) => ui::share_view::view(app),
        Some(Page::OpenAiLive) => ui::openai_live_view::view(app),
        Some(Page::QuotaAlert) => ui::overlay_view::alert_view(app),
        Some(Page::Confetti) => ui::overlay_view::confetti_view(app),
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
    // Agent Sessions auto-refresh every 10 s, only while its window is open.
    // It also runs windowless while Stay Awake is on, so the inhibitor tracks live sessions.
    let sessions_poll =
        if app.extras.stay_awake || app.windows.values().any(|p| *p == Page::Sessions) {
            iced::time::every(Duration::from_secs(10)).map(|_| Message::RefreshSessions)
        } else {
            iced::Subscription::none()
        };
    let storage_poll =
        iced::time::every(Duration::from_secs(storage::REFRESH_SECS)).map(|_| Message::ScanStorage);
    // Redraw every frame only while the confetti animation is on screen.
    let frames = if app.confetti.is_some() {
        iced::window::frames().map(|_| Message::OverlayFrame)
    } else {
        iced::Subscription::none()
    };
    // Dev captures need to know when each window has really been drawn.
    let redrawn = if shots::capturing() {
        iced::event::listen_raw(|event, _, id| match event {
            iced::Event::Window(iced::window::Event::RedrawRequested(_))
                if shots::wants_redraw(id) =>
            {
                Some(Message::Redrawn(id))
            }
            _ => None,
        })
    } else {
        iced::Subscription::none()
    };
    iced::Subscription::batch([
        tick,
        poll,
        closed,
        tray_sub,
        ipc_sub,
        sessions_poll,
        storage_poll,
        frames,
        redrawn,
    ])
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
