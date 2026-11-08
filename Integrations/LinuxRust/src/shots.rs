//! Dev/QA only: `CODEXBAR_SCREENSHOT_DIR=<dir>` saves every open window as PNG (each spend tab
//! and settings pane) once live data has loaded, then quits. Used for README shots and review,
//! since Wayland portals refuse unattended desktop captures.

use crate::state::{App, Message, Page};
use crate::ui::settings_view::ALL_PANES;
use std::cell::Cell;

thread_local! {
    /// (step index, state applied and waiting one tick for a redraw)
    static STEP: Cell<(usize, bool)> = const { Cell::new((0, false)) };
}

pub fn tick(app: &mut App) -> Option<iced::Task<Message>> {
    let dir = std::env::var_os("CODEXBAR_SCREENSHOT_DIR")?;
    let wants_costs = app.windows.values().any(|p| *p == Page::Usage);
    let ready = !app.windows.is_empty()
        && !app.busy
        && !app.updated.is_empty()
        && (!wants_costs || app.cost_updated.is_some() || !app.cost_error.is_empty());
    if !ready {
        return None;
    }
    let steps: Vec<(iced::window::Id, String)> = app
        .windows
        .iter()
        .flat_map(|(id, page)| match page {
            Page::QuickView => vec![(*id, "quick-view".to_string())],
            Page::Usage => ["providers", "models", "ledger"]
                .iter()
                .map(|t| (*id, format!("spend-{t}")))
                .collect(),
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
        return Some(iced::Task::none());
    }
    STEP.set((i + 1, false));
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
