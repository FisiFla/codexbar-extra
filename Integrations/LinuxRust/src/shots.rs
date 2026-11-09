//! Dev/QA only: `CODEXBAR_SCREENSHOT_DIR=<dir>` saves every open window as PNG (each spend tab
//! and settings pane) once live data has loaded, then quits. Used for README shots and review,
//! since Wayland portals refuse unattended desktop captures. It also opens the Share Stats
//! window, captures it, then exercises the real Save PNG path (set XDG_PICTURES_DIR).

use crate::state::{App, Message, Page};
use crate::ui::settings_view::ALL_PANES;
use std::cell::Cell;

thread_local! {
    /// (step index, state applied and waiting one tick for a redraw)
    static STEP: Cell<(usize, bool)> = const { Cell::new((0, false)) };
    static SHARE_OPENED: Cell<bool> = const { Cell::new(false) };
}

pub fn tick(app: &mut App) -> Option<iced::Task<Message>> {
    let dir = std::env::var_os("CODEXBAR_SCREENSHOT_DIR")?;
    let wants_costs = app
        .windows
        .values()
        .any(|p| matches!(p, Page::Usage | Page::QuickView));
    let ready = !app.windows.is_empty()
        && !app.busy
        && (!app.updated.is_empty() || app.windows.values().all(|p| *p == Page::Sessions))
        && (app.sessions_loaded || !app.windows.values().any(|p| *p == Page::Sessions))
        && (!wants_costs || app.cost_updated.is_some() || !app.cost_error.is_empty());
    if !ready {
        return None;
    }
    // Let the async Save PNG finish before the next step or the final exit.
    if app.share_status == "Rendering image…" {
        return Some(iced::Task::none());
    }
    if !SHARE_OPENED.get() {
        SHARE_OPENED.set(true);
        return Some(iced::Task::done(Message::ShowPage(Page::ShareStats)));
    }
    let steps: Vec<(iced::window::Id, String)> = app
        .windows
        .iter()
        .flat_map(|(id, page)| match page {
            Page::QuickView => ["quick-view", "quick-view-cost", "quick-view-models"]
                .iter()
                .map(|n| (*id, n.to_string()))
                .collect::<Vec<_>>(),
            Page::Usage => ["providers", "models", "ledger"]
                .iter()
                .map(|t| (*id, format!("spend-{t}")))
                .collect(),
            Page::Sessions => vec![(*id, "sessions".to_string())],
            Page::ShareStats => vec![
                (*id, "share-stats".to_string()),
                (*id, "share-save".to_string()),
            ],
            Page::Settings => ALL_PANES
                .iter()
                .map(|p| (*id, format!("settings-{p:?}")))
                .chain(std::iter::once((*id, "provider-codex".to_string())))
                .collect(),
        })
        .collect();
    let (i, armed) = STEP.get();
    let Some((id, name)) = steps.get(i).cloned() else {
        return Some(iced::exit());
    };
    if !armed {
        if let Some(chart) = name.strip_prefix("quick-view-") {
            // First provider with daily cost data: expand one chart and pin the hover on its
            // peak bar so the tooltip is visible, then scroll the card to the chart.
            let pick = app.entries.iter().position(|e| {
                app.cost_payloads
                    .iter()
                    .any(|p| p.provider == e.provider && !p.daily.is_empty())
            });
            if let Some(i) = pick {
                let provider = app.entries[i].provider.clone();
                let key = format!(
                    "{provider}:{}",
                    if chart == "cost" { "cost" } else { "models" }
                );
                if let Some(p) = app.cost_payloads.iter().find(|p| p.provider == provider) {
                    let bars = crate::charts::cost_bars(&p.daily);
                    let hover = crate::charts::peak_index(&bars).unwrap_or(0);
                    app.chart_hover = Some((key.clone(), hover));
                }
                app.selected = i as i32;
                app.expanded_charts.clear();
                app.expanded_charts.insert(key);
            }
            STEP.set((i, true));
            return Some(iced::widget::scrollable::snap_to(
                crate::ui::popover::scroll_id(),
                iced::widget::scrollable::RelativeOffset::END,
            ));
        }
        if let Some(tab) = name.strip_prefix("spend-") {
            app.spend_tab = tab.to_string();
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
        STEP.set((i, true));
        if name.starts_with("spend-") {
            // Trend chart sits below the usage cards; scroll it into view.
            return Some(iced::widget::scrollable::snap_to(
                crate::ui::spend_view::scroll_id(),
                iced::widget::scrollable::RelativeOffset { x: 0.0, y: 0.8 },
            ));
        }
        return Some(iced::Task::none());
    }
    STEP.set((i + 1, false));
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
