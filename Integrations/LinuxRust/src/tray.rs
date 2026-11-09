//! StatusNotifierItem tray (ksni), mirroring the Qt tray in
//! `Integrations/Linux/main.cpp`: two-meter icon, shared-state tooltip,
//! and Quick View / Usage & Spend / Settings / Refresh / Quit menu.
//! Activations and menu clicks are bridged to the iced loop as
//! [`crate::state::TrayCommand`] items on the stream from [`stream`].

use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::menubar_layout::{self, MenuBarLayout, RenderInput, TitleMode};
use crate::model::{CostEntry, Entry, QuotaWindow};
use crate::state::{Shared, TrayCommand};

pub const TRAY_ID: &str = "com.steipete.CodexBar";
pub const TRAY_TITLE: &str = "CodexBar";
pub const ICON_NAME: &str = "codexbar";

pub const PIXMAP_SIZE: u32 = 64;
/// Qt `QRectF track(5, 12 + index * 25, 54, 15)`.
pub const TRACK_X: f32 = 5.0;
pub const TRACK_WIDTH: f32 = 54.0;
pub const TRACK_HEIGHT: f32 = 15.0;
pub const TRACK_RADIUS: f32 = 4.0;
pub const TRACK_YS: [f32; 2] = [12.0, 37.0];

pub const TRACK_RGB: (u8, u8, u8) = (0x77, 0x77, 0x77); // #777777
/// Fallback for Qt `palette().highlight()` (system accent blue).
pub const HIGHLIGHT_RGB: (u8, u8, u8) = (0x35, 0x84, 0xe4); // #3584e4
pub const WARNING_RGB: (u8, u8, u8) = (0xe5, 0x96, 0x42); // #e59642
pub const STALE_RGB: (u8, u8, u8) = (0xaa, 0xaa, 0xaa); // #aaaaaa

/// Fraction of the track to fill, exactly like Qt:
/// `value = quotaDisplay == "used" ? 100 - remaining : remaining`,
/// with `remaining` clamped to `0..=100` first.
pub fn meter_fraction(remaining: i64, quota_used: bool) -> f32 {
    let remaining = remaining.clamp(0, 100) as f32;
    let value = if quota_used {
        100.0 - remaining
    } else {
        remaining
    };
    (value / 100.0).clamp(0.0, 1.0)
}

/// Fill width in pixels: `track.width() * value / 100` in Qt.
pub fn meter_fill_width(track_width: f32, remaining: i64, quota_used: bool) -> f32 {
    track_width * meter_fraction(remaining, quota_used)
}

/// Warning when `warningColors && remaining <= notifyThreshold` (Qt),
/// with `remaining` clamped like the meter value.
pub fn is_warning(remaining: i64, warning_colors: bool, notify_threshold: u32) -> bool {
    warning_colors && remaining.clamp(0, 100) <= notify_threshold as i64
}

/// Fill colour with Qt precedence: stale, then warning, then highlight.
pub fn fill_color_rgb(
    remaining: i64,
    stale: bool,
    warning_colors: bool,
    notify_threshold: u32,
) -> (u8, u8, u8) {
    if stale {
        STALE_RGB
    } else if is_warning(remaining, warning_colors, notify_threshold) {
        WARNING_RGB
    } else {
        HIGHLIGHT_RGB
    }
}

/// `CodexBar · {summary} · {quotaDisplay}[ · out of date]`, like Qt.
/// An empty summary reads `Usage unavailable`, as in `main.cpp`.
pub fn tooltip_text(summary: &str, quota_display: &str, stale: bool) -> String {
    let summary = if summary.is_empty() {
        "Usage unavailable"
    } else {
        summary
    };
    let suffix = if stale { " \u{00b7} out of date" } else { "" };
    format!("CodexBar \u{00b7} {summary} \u{00b7} {quota_display}{suffix}")
}

/// Themed icon instead of meters when the user picked `icon` style or
/// there is nothing to draw yet (no quota windows).
pub fn use_icon_name(tray_style: &str, no_windows: bool) -> bool {
    tray_style == "icon" || no_windows
}

/// Left-click target: Quick View in compact mode, Usage otherwise (Qt).
pub fn activate_command(compact_quick_view: bool) -> TrayCommand {
    if compact_quick_view {
        TrayCommand::QuickView
    } else {
        TrayCommand::Usage
    }
}

/// RGBA bytes (tiny-skia order `R,G,B,A` per pixel) to ksni
/// ARGB32 in network byte order (`A,R,G,B` per pixel).
/// Trailing bytes that do not form a full pixel are dropped.
pub fn rgba_to_argb(rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.len() / 4 * 4);
    for px in rgba.chunks_exact(4) {
        out.push(px[3]);
        out.push(px[0]);
        out.push(px[1]);
        out.push(px[2]);
    }
    out
}

/// Two-meter 64x64 icon as RGBA bytes (`64 * 64 * 4`, row-major).
/// Uses the settings defaults for warning colouring
/// (`warningColors=true`, `notifyThreshold=10`); the tray itself
/// calls [`meters_pixmap_with`] with the live settings.
/// Pixels are opaque or fully transparent, so tiny-skia's
/// premultiplied storage equals straight RGBA here.
pub fn meters_pixmap(windows: &[QuotaWindow], stale: bool, quota_used: bool) -> Vec<u8> {
    meters_pixmap_with(windows, stale, quota_used, true, 10)
}

/// Full form of [`meters_pixmap`] with live warning settings.
/// Draws the two Qt tracks even with fewer windows (track only);
/// the tray switches to the themed icon when there are none.
pub fn meters_pixmap_with(
    windows: &[QuotaWindow],
    stale: bool,
    quota_used: bool,
    warning_colors: bool,
    notify_threshold: u32,
) -> Vec<u8> {
    let bars: Vec<Option<i64>> = windows.iter().map(|w| Some(w.remaining)).collect();
    bars_pixmap(&bars, stale, quota_used, warning_colors, notify_threshold)
}

/// Draws up to two tracks; `None` (unknown value) leaves the track empty
/// instead of drawing a fabricated 0 or 100.
pub fn bars_pixmap(
    bars: &[Option<i64>],
    stale: bool,
    quota_used: bool,
    warning_colors: bool,
    notify_threshold: u32,
) -> Vec<u8> {
    let mut pixmap = match tiny_skia::Pixmap::new(PIXMAP_SIZE, PIXMAP_SIZE) {
        Some(pixmap) => pixmap,
        None => return vec![0; (PIXMAP_SIZE * PIXMAP_SIZE * 4) as usize],
    };
    for (index, y) in TRACK_YS.iter().copied().enumerate() {
        fill_rounded_rect(
            &mut pixmap,
            TRACK_X,
            y,
            TRACK_WIDTH,
            TRACK_HEIGHT,
            TRACK_RADIUS,
            TRACK_RGB,
        );
        let Some(Some(remaining)) = bars.get(index).copied() else {
            continue;
        };
        let fill_width = meter_fill_width(TRACK_WIDTH, remaining, quota_used);
        if fill_width <= 0.0 {
            continue;
        }
        let rgb = fill_color_rgb(remaining, stale, warning_colors, notify_threshold);
        fill_rounded_rect(
            &mut pixmap,
            TRACK_X,
            y,
            fill_width,
            TRACK_HEIGHT,
            TRACK_RADIUS,
            rgb,
        );
    }
    pixmap.data().to_vec()
}

fn fill_rounded_rect(
    pixmap: &mut tiny_skia::Pixmap,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    radius: f32,
    rgb: (u8, u8, u8),
) {
    let Some(path) = rounded_rect_path(x, y, width, height, radius) else {
        return;
    };
    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8(rgb.0, rgb.1, rgb.2, 0xff);
    paint.anti_alias = true;
    pixmap.fill_path(
        &path,
        &paint,
        tiny_skia::FillRule::Winding,
        tiny_skia::Transform::identity(),
        None,
    );
}

fn rounded_rect_path(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    radius: f32,
) -> Option<tiny_skia::Path> {
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let radius = radius.min(width / 2.0).min(height / 2.0).max(0.0);
    if radius <= 0.01 {
        let rect = tiny_skia::Rect::from_xywh(x, y, width, height)?;
        return Some(tiny_skia::PathBuilder::from_rect(rect));
    }
    // Cubic approximation of each quarter circle (kappa = 0.5522847498).
    let k = 0.5522847498 * radius;
    let (r, right, bottom) = (radius, x + width, y + height);
    let mut b = tiny_skia::PathBuilder::new();
    b.move_to(x + r, y);
    b.line_to(right - r, y);
    b.cubic_to(right - r + k, y, right, y + r - k, right, y + r);
    b.line_to(right, bottom - r);
    b.cubic_to(
        right,
        bottom - r + k,
        right - r + k,
        bottom,
        right - r,
        bottom,
    );
    b.line_to(x + r, bottom);
    b.cubic_to(x + r - k, bottom, x, bottom - r + k, x, bottom - r);
    b.line_to(x, y + r);
    b.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    b.close();
    b.finish()
}

pub enum TrayIcon {
    /// Themed icon by name (`ICON_NAME`), no pixmap.
    Named,
    /// 64x64 RGBA pixmap.
    Pixmap(Vec<u8>),
}

/// What the tray shows for a layout: icon, SNI title and tooltip description.
pub struct TrayView {
    pub icon: TrayIcon,
    pub title: String,
    pub description: String,
}

pub struct TrayParams<'a> {
    pub tray_style: &'a str,
    pub stale: bool,
    pub quota_used: bool,
    pub warning_colors: bool,
    pub notify_threshold: u32,
}

/// Single entry point shared by the tray and the editor preview.
/// With the default layout (`[[icon]]`, static title) this is exactly the
/// pre-layout behaviour: meters or themed icon, title `CodexBar`, no description.
pub fn tray_view(
    layout: &MenuBarLayout,
    entry: Option<&Entry>,
    cost: Option<&CostEntry>,
    params: &TrayParams,
    now: chrono::DateTime<chrono::Utc>,
) -> TrayView {
    let rendered = menubar_layout::render(
        layout,
        &RenderInput {
            entry,
            cost,
            quota_used: params.quota_used,
            now,
        },
    );
    // Qt reads the first entry's windows for the tray meters.
    let windows: Vec<QuotaWindow> = entry
        .map(|e| e.windows.iter().take(2).cloned().collect())
        .unwrap_or_default();
    let bars = rendered.bars();
    let icon = if rendered.has_icon() {
        if use_icon_name(params.tray_style, windows.is_empty()) {
            TrayIcon::Named
        } else {
            TrayIcon::Pixmap(meters_pixmap_with(
                &windows,
                params.stale,
                params.quota_used,
                params.warning_colors,
                params.notify_threshold,
            ))
        }
    } else if !bars.is_empty() {
        TrayIcon::Pixmap(bars_pixmap(
            &bars,
            params.stale,
            params.quota_used,
            params.warning_colors,
            params.notify_threshold,
        ))
    } else {
        TrayIcon::Named
    };
    let text = rendered.title_text();
    let (title, description) = match layout.title_mode {
        TitleMode::Layout if !text.is_empty() => (text.clone(), text),
        _ => (TRAY_TITLE.to_string(), String::new()),
    };
    TrayView {
        icon,
        title,
        description,
    }
}

/// Cached view of [`Shared`] owned by the tray; refreshed by the
/// 2-second poller through the ksni handle so `Tray` getters never lock.
#[derive(Clone, PartialEq)]
struct TraySnapshot {
    windows: Vec<QuotaWindow>,
    stale: bool,
    quota_used: bool,
    quota_display: String,
    summary: String,
    warning_colors: bool,
    notify_threshold: u32,
    tray_style: String,
    show_tray: bool,
    compact_quick_view: bool,
    layout: MenuBarLayout,
    entry: Option<Entry>,
    cost: Option<CostEntry>,
}

impl TraySnapshot {
    fn from_shared(shared: &Shared) -> Self {
        // Qt reads the first entry's windows for the tray meters.
        let windows = shared
            .entries
            .first()
            .map(|entry| entry.windows.iter().take(2).cloned().collect())
            .unwrap_or_default();
        let entry = shared.entries.first().cloned();
        let cost = entry
            .as_ref()
            .and_then(|e| shared.spending.iter().find(|c| c.provider == e.provider))
            .cloned();
        Self {
            windows,
            layout: shared.menubar_layout.clone(),
            entry,
            cost,
            stale: shared.stale,
            quota_used: shared.quota_used,
            quota_display: shared.settings.quota_display.clone(),
            summary: shared.summary.clone(),
            warning_colors: shared.settings.warning_colors,
            notify_threshold: shared.settings.notify_threshold,
            tray_style: shared.settings.tray_style.clone(),
            show_tray: shared.settings.show_tray,
            compact_quick_view: shared.settings.compact_quick_view,
        }
    }

    fn view(&self) -> TrayView {
        tray_view(
            &self.layout,
            self.entry.as_ref(),
            self.cost.as_ref(),
            &TrayParams {
                tray_style: &self.tray_style,
                stale: self.stale,
                quota_used: self.quota_used,
                warning_colors: self.warning_colors,
                notify_threshold: self.notify_threshold,
            },
            chrono::Utc::now(),
        )
    }
}

fn snapshot_of(shared: &Arc<Mutex<Shared>>) -> TraySnapshot {
    match shared.lock() {
        Ok(shared) => TraySnapshot::from_shared(&shared),
        Err(poisoned) => TraySnapshot::from_shared(&poisoned.into_inner()),
    }
}

struct CodexTray {
    tx: iced::futures::channel::mpsc::UnboundedSender<TrayCommand>,
    shared: Arc<Mutex<Shared>>,
    snapshot: TraySnapshot,
}

impl CodexTray {
    fn send(&self, command: TrayCommand) {
        // A closed receiver means the iced subscription is gone; the
        // tray must not panic the D-Bus loop over a dead UI bridge.
        if self.tx.unbounded_send(command).is_err() {
            eprintln!("CodexBar tray: UI bridge closed; ignoring tray activation");
        }
    }

    fn compact_quick_view_now(&self) -> bool {
        match self.shared.lock() {
            Ok(shared) => shared.settings.compact_quick_view,
            Err(poisoned) => poisoned.into_inner().settings.compact_quick_view,
        }
    }
}

impl ksni::Tray for CodexTray {
    fn id(&self) -> String {
        TRAY_ID.to_string()
    }

    fn title(&self) -> String {
        self.snapshot.view().title
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::ApplicationStatus
    }

    fn status(&self) -> ksni::Status {
        if self.snapshot.show_tray {
            ksni::Status::Active
        } else {
            ksni::Status::Passive
        }
    }

    fn icon_name(&self) -> String {
        match self.snapshot.view().icon {
            TrayIcon::Named => ICON_NAME.to_string(),
            TrayIcon::Pixmap(_) => String::new(),
        }
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        match self.snapshot.view().icon {
            TrayIcon::Named => Vec::new(),
            TrayIcon::Pixmap(rgba) => vec![ksni::Icon {
                width: PIXMAP_SIZE as i32,
                height: PIXMAP_SIZE as i32,
                data: rgba_to_argb(&rgba),
            }],
        }
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let view = self.snapshot.view();
        ksni::ToolTip {
            icon_name: match view.icon {
                TrayIcon::Named => ICON_NAME.to_string(),
                TrayIcon::Pixmap(_) => String::new(),
            },
            icon_pixmap: Vec::new(),
            title: tooltip_text(
                &self.snapshot.summary,
                &self.snapshot.quota_display,
                self.snapshot.stale,
            ),
            description: view.description,
        }
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;
        vec![
            StandardItem {
                label: "Quick View".into(),
                activate: Box::new(|tray: &mut Self| tray.send(TrayCommand::QuickView)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Usage & Spend\u{2026}".into(),
                activate: Box::new(|tray: &mut Self| tray.send(TrayCommand::Usage)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Agent Sessions\u{2026}".into(),
                activate: Box::new(|tray: &mut Self| tray.send(TrayCommand::Sessions)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Share Stats\u{2026}".into(),
                activate: Box::new(|tray: &mut Self| tray.send(TrayCommand::ShareStats)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Settings\u{2026}".into(),
                activate: Box::new(|tray: &mut Self| tray.send(TrayCommand::Settings)),
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            StandardItem {
                label: "Refresh".into(),
                activate: Box::new(|tray: &mut Self| tray.send(TrayCommand::Refresh)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Quit CodexBar".into(),
                activate: Box::new(|tray: &mut Self| tray.send(TrayCommand::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        let command = activate_command(self.compact_quick_view_now());
        self.send(command);
    }

    // ksni 0.2 spells this hook `watcher_offine`.
    fn watcher_offine(&self) -> bool {
        eprintln!("CodexBar tray: no StatusNotifierWatcher; tray hidden until one appears");
        true
    }
}

/// Iced subscription stream for the tray. Spawns the blocking ksni
/// service (plus its 2-second snapshot poller) on dedicated std
/// threads and bridges commands over an mpsc channel via `unfold`.
/// If the service cannot start (no session bus / watcher) it logs
/// to stderr and the stream stays pending: a guard sender kept in
/// the unfold state holds the channel open.
pub fn stream(
    shared: Arc<Mutex<Shared>>,
) -> iced::futures::stream::BoxStream<'static, TrayCommand> {
    use iced::futures::StreamExt as _;

    let (tx, rx) = iced::futures::channel::mpsc::unbounded::<TrayCommand>();
    let guard = tx.clone();
    let thread_shared = shared.clone();
    let spawned = std::thread::Builder::new()
        .name("codexbar-tray".into())
        .spawn(move || {
            let tray = CodexTray {
                tx,
                shared: thread_shared.clone(),
                snapshot: snapshot_of(&thread_shared),
            };
            let service = ksni::TrayService::new(tray);
            let handle = service.handle();
            let poll_shared = thread_shared.clone();
            let poll_spawned = std::thread::Builder::new()
                .name("codexbar-tray-poll".into())
                .spawn(move || loop {
                    std::thread::sleep(Duration::from_secs(2));
                    let snapshot = snapshot_of(&poll_shared);
                    handle.update(|tray: &mut CodexTray| tray.snapshot = snapshot);
                });
            if let Err(error) = poll_spawned {
                eprintln!("CodexBar tray: failed to start snapshot poller: {error}");
            }
            if let Err(error) = service.run() {
                eprintln!("CodexBar tray: failed to start StatusNotifierItem service: {error}");
            }
        });
    if let Err(error) = spawned {
        eprintln!("CodexBar tray: failed to spawn tray thread: {error}");
    }

    Box::pin(iced::futures::stream::unfold(
        (rx, guard),
        |(mut rx, guard)| async move { rx.next().await.map(|command| (command, (rx, guard))) },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::QuotaWindow;
    use crate::state::TrayCommand;

    fn window(remaining: i64) -> QuotaWindow {
        QuotaWindow {
            key: "primary".into(),
            label: "Session".into(),
            minutes: 300,
            remaining,
            resets_at: String::new(),
            reset_text: String::new(),
            pace: String::new(),
            pace_expected: 0.0,
            pace_delta: 0.0,
            scoped: false,
        }
    }

    fn px(rgba: &[u8], x: usize, y: usize) -> [u8; 4] {
        let i = (y * 64 + x) * 4;
        [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
    }

    #[test]
    fn meter_fraction_remaining_and_used() {
        assert!((meter_fraction(80, false) - 0.8).abs() < 1e-6);
        assert!((meter_fraction(80, true) - 0.2).abs() < 1e-6);
        assert!((meter_fraction(100, false) - 1.0).abs() < 1e-6);
        assert!((meter_fraction(0, false) - 0.0).abs() < 1e-6);
        assert!((meter_fraction(0, true) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn meter_fraction_clamps() {
        assert!((meter_fraction(150, false) - 1.0).abs() < 1e-6);
        assert!((meter_fraction(-20, false) - 0.0).abs() < 1e-6);
        assert!((meter_fraction(150, true) - 0.0).abs() < 1e-6);
        assert!((meter_fraction(-20, true) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn meter_fill_width_math() {
        // Qt: track width 54, fill = 54 * value / 100.
        assert!((meter_fill_width(TRACK_WIDTH, 50, false) - 27.0).abs() < 1e-4);
        assert!((meter_fill_width(TRACK_WIDTH, 50, true) - 27.0).abs() < 1e-4);
        assert!((meter_fill_width(TRACK_WIDTH, 25, false) - 13.5).abs() < 1e-4);
        assert!((meter_fill_width(TRACK_WIDTH, 25, true) - 40.5).abs() < 1e-4);
        assert!((meter_fill_width(TRACK_WIDTH, 100, false) - 54.0).abs() < 1e-4);
        assert!((meter_fill_width(TRACK_WIDTH, 0, false) - 0.0).abs() < 1e-4);
    }

    #[test]
    fn warning_color_choice() {
        assert_eq!(fill_color_rgb(80, false, true, 10), (0x35, 0x84, 0xe4));
        assert_eq!(fill_color_rgb(10, false, true, 10), (0xe5, 0x96, 0x42));
        assert_eq!(fill_color_rgb(5, false, true, 10), (0xe5, 0x96, 0x42));
        assert_eq!(fill_color_rgb(5, false, false, 10), (0x35, 0x84, 0xe4));
        assert_eq!(fill_color_rgb(11, false, true, 10), (0x35, 0x84, 0xe4));
        assert_eq!(fill_color_rgb(5, true, true, 10), (0xaa, 0xaa, 0xaa));
        assert_eq!(fill_color_rgb(80, true, true, 10), (0xaa, 0xaa, 0xaa));
        assert!(is_warning(10, true, 10));
        assert!(!is_warning(11, true, 10));
        assert!(!is_warning(5, false, 10));
    }

    #[test]
    fn tooltip_format() {
        assert_eq!(
            tooltip_text("CX 80%", "remaining", false),
            "CodexBar \u{00b7} CX 80% \u{00b7} remaining"
        );
        assert_eq!(
            tooltip_text("CX 80%", "used", true),
            "CodexBar \u{00b7} CX 80% \u{00b7} used \u{00b7} out of date"
        );
        assert_eq!(
            tooltip_text("", "remaining", false),
            "CodexBar \u{00b7} Usage unavailable \u{00b7} remaining"
        );
    }

    #[test]
    fn icon_and_activate_logic() {
        assert!(use_icon_name("icon", false));
        assert!(use_icon_name("meters", true));
        assert!(!use_icon_name("meters", false));
        assert_eq!(activate_command(true), TrayCommand::QuickView);
        assert_eq!(activate_command(false), TrayCommand::Usage);
    }

    #[test]
    fn rgba_to_argb_conversion() {
        let rgba = [0x11, 0x22, 0x33, 0xff, 0x00, 0x00, 0x00, 0x00];
        assert_eq!(
            rgba_to_argb(&rgba),
            vec![0xff, 0x11, 0x22, 0x33, 0, 0, 0, 0]
        );
    }

    #[test]
    fn meters_pixmap_size_and_tracks() {
        let rgba = meters_pixmap(&[window(100), window(0)], false, false);
        assert_eq!(rgba.len(), 64 * 64 * 4);
        assert_eq!(px(&rgba, 0, 0), [0, 0, 0, 0]);
        assert_eq!(px(&rgba, 55, 19), [0x35, 0x84, 0xe4, 0xff]);
        assert_eq!(px(&rgba, 55, 44), [0x77, 0x77, 0x77, 0xff]);
    }

    #[test]
    fn meters_pixmap_warning_stale_and_used() {
        // Low remaining with warning colours -> orange near the left edge.
        let warn = meters_pixmap_with(&[window(5)], false, false, true, 10);
        assert_eq!(px(&warn, 6, 19), [0xe5, 0x96, 0x42, 0xff]);
        // Stale wins over warning.
        let stale = meters_pixmap_with(&[window(5)], true, false, true, 10);
        assert_eq!(px(&stale, 6, 19), [0xaa, 0xaa, 0xaa, 0xff]);
        // Quota-used inverts the fill: 100% remaining draws no fill.
        let used = meters_pixmap(&[window(100)], false, true);
        assert_eq!(px(&used, 55, 19), [0x77, 0x77, 0x77, 0xff]);
        // No windows still draws bare tracks (tray uses icon_name instead).
        let empty = meters_pixmap(&[], false, false);
        assert_eq!(empty.len(), 64 * 64 * 4);
        assert_eq!(px(&empty, 32, 19), [0x77, 0x77, 0x77, 0xff]);
    }
}
