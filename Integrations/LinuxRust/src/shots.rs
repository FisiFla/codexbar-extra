//! Dev/QA only: `CODEXBAR_SCREENSHOT_DIR=<dir>` saves every open window as PNG (each spend tab
//! and settings pane) once live data has loaded, then quits. Used for README shots and review,
//! since Wayland portals refuse unattended desktop captures. It also opens the Share Stats
//! window, captures it, then exercises the real Save PNG path (set XDG_PICTURES_DIR).
//! `CODEXBAR_FORCE_OVERLAY=1` additionally opens the quota alert and confetti overlays.
//!
//! Race fix: a state change (pane, tab, scroll) only shows its text a frame or two later, since
//! glyphs are shaped and uploaded on the first draw after the view changes. Every state change
//! therefore waits `SETTLE_TICKS` ticks (each a full update + redraw round) before the capture,
//! instead of capturing on the very next tick.

use crate::state::{App, Message, Page};
use crate::ui::settings_view::ALL_PANES;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

thread_local! {
    /// (step index, ticks waited since this step's state was applied; 0 = not applied yet)
    static STEP: Cell<(usize, u8)> = const { Cell::new((0, 0)) };
    /// Redraws seen per window, and the target window's count when the step's state was applied.
    static REDRAWS: RefCell<HashMap<iced::window::Id, u64>> = RefCell::new(HashMap::new());
    static BASELINE: Cell<u64> = const { Cell::new(0) };
    static SHARE_OPENED: Cell<bool> = const { Cell::new(false) };
    static OVERLAYS_OPENED: Cell<bool> = const { Cell::new(false) };
    static LIVE_OPENED: Cell<bool> = const { Cell::new(false) };
}

/// Redraws of the target window (after its state was applied) required before a capture, and
/// the longest wait for them. A window the compositor keeps covered or occluded gets no frame
/// callbacks and can sit unrendered for many seconds, so the wait is generous.
const SETTLE_REDRAWS: u64 = 2;
const MAX_WAIT_TICKS: u8 = 40;

pub fn capturing() -> bool {
    std::env::var_os("CODEXBAR_SCREENSHOT_DIR").is_some()
}

/// `--usage` run: the Settings window opens on the Spend pane and steps through the dashboard
/// sections instead of the settings panes.
fn usage_run() -> bool {
    std::env::args().any(|a| a == "--usage")
}

/// Dashboard capture steps: name and the scroll offset of the section under test.
const SPEND_STEPS: &[(&str, f32)] = &[
    ("top", 0.0),
    ("providers", 300.0),
    ("models", 300.0),
    ("projects", 300.0),
    ("chats", 300.0),
    ("sessions", 300.0),
    ("trend-overview", 870.0),
    ("trend-hour", 870.0),
    ("trend-selected", 870.0),
    ("ledger-top", 1430.0),
    ("ledger-bottom", 1900.0),
    ("heatmap", 3000.0),
    ("data-controls", 4000.0),
];

/// The window whose redraws are being waited for, and whether enough have been seen. These are
/// statics (not thread-locals) because the subscription filter runs on the runtime thread.
static TARGET: Mutex<Option<iced::window::Id>> = Mutex::new(None);
static SETTLED: AtomicBool = AtomicBool::new(true);

/// Subscription filter: only the target window's redraws are worth a message (each message
/// requests another redraw of every window, so anything broader would spin).
pub fn wants_redraw(id: iced::window::Id) -> bool {
    !SETTLED.load(Ordering::Relaxed) && *TARGET.lock().unwrap() == Some(id)
}

/// Called after every draw of the target window (iced broadcasts `RedrawRequested` right
/// after `ui.draw`).
pub fn on_redraw(id: iced::window::Id) {
    let n = REDRAWS.with(|r| {
        let mut r = r.borrow_mut();
        let n = r.entry(id).or_default();
        *n += 1;
        *n
    });
    if n.saturating_sub(BASELINE.get()) >= SETTLE_REDRAWS {
        SETTLED.store(true, Ordering::Relaxed);
    }
}

fn redraws(id: iced::window::Id) -> u64 {
    REDRAWS.with(|r| r.borrow().get(&id).copied().unwrap_or(0))
}

pub fn tick(app: &mut App) -> Option<iced::Task<Message>> {
    let dir = std::env::var_os("CODEXBAR_SCREENSHOT_DIR")?;
    let wants_costs = app
        .windows
        .values()
        .any(|p| matches!(p, Page::Usage | Page::QuickView) || (usage_run() && *p == Page::Settings));
    let wants_scan = app
        .windows
        .values()
        .any(|p| *p == Page::Usage || (usage_run() && *p == Page::Settings));
    let ready = !app.windows.is_empty()
        && !app.busy
        && (!app.updated.is_empty() || app.windows.values().all(|p| *p == Page::Sessions))
        && (app.sessions_loaded || !app.windows.values().any(|p| *p == Page::Sessions))
        && app.storage_loaded
        && (!wants_costs || app.cost_updated.is_some() || !app.cost_error.is_empty())
        && (!wants_scan || app.scan_loaded || !app.settings.show_costs);
    if !ready {
        return None;
    }
    // Let the async Save PNG finish before the next step or the final exit.
    if app.share_status == "Rendering image…" {
        return Some(iced::Task::none());
    }
    if crate::overlay::forced() && !OVERLAYS_OPENED.get() {
        OVERLAYS_OPENED.set(true);
        return Some(iced::Task::done(Message::ForceOverlays));
    }
    if !LIVE_OPENED.get() {
        LIVE_OPENED.set(true);
        return Some(iced::Task::done(Message::ShowPage(Page::OpenAiLive)));
    }
    let steps: Vec<(iced::window::Id, String)> = app
        .windows
        .iter()
        // The Share window opens last: while it covers the others the compositor stops sending
        // them frame callbacks, so they would never redraw after a state change.
        .filter(|(_, page)| **page != Page::ShareStats || SHARE_OPENED.get())
        .flat_map(|(id, page)| match page {
            Page::QuickView => [
                "quick-view",
                "quick-view-util",
                "quick-view-burn",
                "quick-view-tpm",
                "quick-view-cost",
                "quick-view-credits",
                "quick-view-models",
                "quick-view-storage",
            ]
            .iter()
            .map(|n| (*id, n.to_string()))
            .collect::<Vec<_>>(),
            Page::Usage => Vec::new(),
            Page::Settings if usage_run() => SPEND_STEPS
                .iter()
                .map(|(n, _)| (*id, format!("spend-{n}")))
                .collect(),
            Page::Sessions => vec![(*id, "sessions".to_string())],
            Page::QuotaAlert => vec![(*id, "overlay-alert".to_string())],
            Page::Confetti => vec![(*id, "overlay-confetti".to_string())],
            Page::ShareStats => vec![
                (*id, "share-stats".to_string()),
                (*id, "share-save".to_string()),
            ],
            Page::OpenAiLive => [
                "all",
                "model",
                "empty",
                "loading",
                "missing",
                "unauthorized",
            ]
            .iter()
            .map(|n| (*id, format!("openai-live-{n}")))
            .collect(),
            Page::Settings => ALL_PANES
                .iter()
                .map(|p| (*id, format!("settings-{p:?}")))
                .chain(std::iter::once((*id, "provider-codex".to_string())))
                .collect(),
        })
        .collect();
    let (i, waited) = STEP.get();
    let Some((id, name)) = steps.get(i).cloned() else {
        if !SHARE_OPENED.get() {
            SHARE_OPENED.set(true);
            STEP.set((i, 0));
            return Some(iced::Task::done(Message::ShowPage(Page::ShareStats)));
        }
        return Some(iced::exit());
    };
    if waited == 0 {
        STEP.set((i, 1));
        BASELINE.set(redraws(id));
        *TARGET.lock().unwrap() = Some(id);
        SETTLED.store(false, Ordering::Relaxed);
        // Bring the target window to the front: a covered window gets no frame callbacks from the
        // compositor and would never redraw the new state.
        let raise = iced::window::gain_focus(id);
        if let Some(state) = name.strip_prefix("openai-live-") {
            apply_live_state(app, state);
            return Some(raise);
        }
        if let Some(chart) = name.strip_prefix("quick-view-") {
            // First provider that has data for this chart: expand it and pin the hover on its
            // peak bar so the tooltip is visible, then scroll the card to the chart.
            let has_cost = |provider: &str| {
                app.cost_payloads
                    .iter()
                    .any(|p| p.provider == provider && !p.daily.is_empty())
            };
            let pick = app.entries.iter().position(|e| match chart {
                "util" | "burn" => !app.history.series_for(&e.provider).is_empty(),
                "credits" => app.history.credit_bars(&e.provider, 0).is_some(),
                "storage" => app.storage.contains_key(&e.provider),
                _ => has_cost(&e.provider),
            });
            if let Some(i) = pick {
                let provider = app.entries[i].provider.clone();
                let key = format!("{provider}:{chart}");
                let payload = app.cost_payloads.iter().find(|p| p.provider == provider);
                let bars = match chart {
                    "cost" => payload.map(|p| crate::charts::cost_bars(&p.daily)),
                    "models" => payload.map(|p| crate::charts::model_bars(&p.daily, 5).0),
                    "tpm" => payload.map(|p| crate::throughput::daily_bars(&p.daily)),
                    "util" => app.history.series_for(&provider).first().map(|s| {
                        let tz = chrono::Local::now().offset().local_minus_utc() as i64;
                        crate::history::utilization_bars(s, tz)
                    }),
                    "credits" => {
                        let tz = chrono::Local::now().offset().local_minus_utc() as i64;
                        app.history.credit_bars(&provider, tz)
                    }
                    _ => None,
                };
                if let Some(bars) = bars {
                    let hover = crate::charts::peak_index(&bars).unwrap_or(0);
                    app.chart_hover = Some((key.clone(), hover));
                }
                app.selected = i as i32;
                app.expanded_charts.clear();
                app.expanded_charts.insert(key);
            }
            return Some(iced::Task::batch([
                raise,
                iced::widget::scrollable::snap_to(
                    crate::ui::popover::scroll_id(),
                    iced::widget::scrollable::RelativeOffset::END,
                ),
            ]));
        }
        if let Some(step) = name.strip_prefix("spend-") {
            return Some(iced::Task::batch([raise, apply_spend_step(app, step)]));
        }
        if let Some(provider) = name.strip_prefix("provider-") {
            app.selected_provider = Some(provider.to_string());
            app.settings_pane = crate::state::SettingsPane::Providers;
        }
        if let Some(pane) = name.strip_prefix("settings-") {
            app.selected_provider = None;
            if let Some(p) = ALL_PANES.iter().find(|p| format!("{p:?}") == pane) {
                app.settings_pane = *p;
            }
        }
        return Some(raise);
    }
    // The capture reads the layers of the last draw, whose text holds weak references to the
    // previous widget tree. Right after a state change they have quads but no glyphs, so wait
    // until the window itself has been redrawn since the change.
    let rendered = redraws(id).saturating_sub(BASELINE.get()) >= SETTLE_REDRAWS;
    if !rendered && waited < MAX_WAIT_TICKS {
        STEP.set((i, waited + 1));
        return Some(iced::Task::none());
    }
    if !rendered {
        eprintln!("screenshot {name}: window never redrew after the state change");
    }
    STEP.set((i + 1, 0));
    if name == "share-save" {
        return Some(crate::share_capture(app, crate::state::ShareAction::Save));
    }
    let path = std::path::PathBuf::from(dir).join(format!("{name}.png"));
    Some(iced::window::screenshot(id).map(move |shot| {
        let png = tiny_skia::IntSize::from_wh(shot.size.width, shot.size.height)
            .and_then(|size| tiny_skia::Pixmap::from_vec(shot.bytes.to_vec(), size))
            .and_then(|pixmap| pixmap.encode_png().ok());
        match png {
            Some(bytes) => {
                let _ = std::fs::write(&path, bytes);
            }
            None => eprintln!("screenshot encode failed: {}", path.display()),
        }
        Message::None
    }))
}

/// Put the dashboard into the state `step` captures and scroll to its section.
fn apply_spend_step(app: &mut App, step: &str) -> iced::Task<Message> {
    use crate::ui::{spend_charts as sc, spend_view};
    app.settings_pane = crate::state::SettingsPane::Spend;
    app.spend_ui = Default::default();
    app.chart_hover = None;
    app.selected_session = None;
    app.spend_tab = "providers".into();
    let today = chrono::Local::now().date_naive();
    match step {
        "providers" | "models" | "projects" | "chats" | "sessions" => {
            app.spend_tab = step.to_string();
            if step == "sessions" {
                // Open the detail pane on the most expensive session.
                app.selected_session = crate::ui::spend_sessions::browse_sessions(
                    &app.scan_view.sessions,
                    "",
                    crate::state::SessionSort::Cost,
                )
                .first()
                .map(|s| (s.provider, s.id.clone()));
            }
        }
        "trend-hour" => app.spend_ui.hour = true,
        "trend-selected" => {
            // Pin the most expensive day and hover the heatmap-free chart on it.
            app.spend_ui.pin = app
                .spend
                .days
                .iter()
                .filter(|d| d.1.is_finite())
                .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                .and_then(|d| spend_view::parse_date(&d.0));
        }
        "ledger-top" | "ledger-bottom" => {
            app.spend_ui.expanded.insert("ledger".into());
        }
        "heatmap" => {
            let grid = sc::activity_grid(&app.spend.days, today);
            if let Some(c) = grid.cells.iter().max_by_key(|c| c.tokens.unwrap_or(-1)) {
                app.chart_hover = Some(("spend-heat".into(), c.col * 7 + c.row));
            }
        }
        _ => {}
    }
    let y = SPEND_STEPS
        .iter()
        .find(|(n, _)| *n == step)
        .map(|(_, y)| *y)
        .unwrap_or(0.0);
    iced::widget::scrollable::scroll_to(
        spend_view::scroll_id(),
        iced::widget::scrollable::AbsoluteOffset { x: 0.0, y },
    )
}

/// Synthetic traffic (no real data): three models with a slow wave, a cached-input share and
/// a quiet gap, ending at the current minute.
fn demo_snapshot(window: i64, quiet: bool) -> crate::openai_live::Snapshot {
    use crate::openai_live::{build, Minute, ModelUsage};
    let now = chrono::Utc::now().timestamp();
    let current = now.div_euclid(60) * 60;
    let mut minutes = Vec::new();
    if !quiet {
        for i in 0..window {
            let start = current - i * 60;
            let wave = |k: i64, base: i64| {
                let phase = ((i * 7 + k * 13) % 29) as f64 / 29.0;
                (base as f64 * (0.2 + phase)) as i64
            };
            let usage = |name: &str, base: i64, k: i64| {
                let input = wave(k, base);
                ModelUsage {
                    name: name.into(),
                    input_tokens: input,
                    cached_input_tokens: input / 3,
                    output_tokens: input / 5,
                    requests: 1 + input / 4000,
                    ..Default::default()
                }
            };
            if i % 11 == 4 {
                continue;
            }
            minutes.push(Minute {
                start,
                models: vec![
                    usage("gpt-4.1", 14000, 1),
                    usage("gpt-4o-mini", 6000, 2),
                    usage("o4-mini", 3000, 3),
                ],
            });
        }
    }
    build(&minutes, window, now, None).expect("demo snapshot")
}

fn apply_live_state(app: &mut App, state: &str) {
    use crate::openai_live::LiveError;
    let live = &mut app.live;
    live.filter = None;
    live.loading = false;
    live.error = None;
    live.snapshot = None;
    live.last_updated = None;
    app.chart_hover = None;
    match state {
        "all" | "model" => {
            let snap = demo_snapshot(live.window_minutes, false);
            live.last_updated = Some(snap.fetched_at - 95);
            if state == "model" {
                live.filter = Some("gpt-4.1".into());
            }
            live.snapshot = Some(snap);
            // Pin the tooltip on a reported minute.
            app.chart_hover = Some(("openai-live".into(), (live.window_minutes as usize) / 2));
        }
        "empty" => {
            let snap = demo_snapshot(live.window_minutes, true);
            live.last_updated = Some(snap.fetched_at - 20);
            live.snapshot = Some(snap);
        }
        "loading" => live.loading = true,
        "missing" => live.error = Some(LiveError::MissingCredential),
        _ => live.error = Some(LiveError::AdminKeyRequired),
    }
}
