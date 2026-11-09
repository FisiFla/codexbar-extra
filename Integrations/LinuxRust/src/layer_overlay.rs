//! Quota alert + reset confetti as real desktop overlays through `zwlr_layer_shell_v1` (COSMIC,
//! KDE and wlroots compositors; GNOME and X11 lack it and keep the iced windows from
//! `ui/overlay_view.rs`). Each overlay runs on its own thread with its own Wayland connection,
//! draws into `wl_shm` buffers with tiny-skia and rasterizes text with cosmic-text. The confetti
//! has an empty input region so clicks pass through; the alert dismisses on click or after
//! `ALERT_SECS`. Any failure before the surface exists reports `false` so the caller falls back
//! to the iced windows.

use crate::overlay::{self, Alert, ALERT_SECS, CONFETTI_SECS};
use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache, Weight};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    delegate_compositor, delegate_layer, delegate_output, delegate_pointer, delegate_registry,
    delegate_seat, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, QueueHandle,
};

const ALERT_WIDTH: f32 = 480.0;
const ALERT_MARGIN_TOP: i32 = 48;
const CARD_INSET: f32 = 8.0;
const CARD_PAD_X: f32 = 20.0;
const CARD_PAD_Y: f32 = 16.0;
const TITLE_LINE: f32 = 24.0;
const MESSAGE_SIZE: f32 = 14.0;
const MESSAGE_LINE: f32 = 18.0;
/// How long `show_*` waits for connect/bind before reporting failure.
const SETUP_TIMEOUT: Duration = Duration::from_secs(2);

fn dev_debug() -> bool {
    std::env::var_os("CODEXBAR_LAYER_OVERLAY_TEST").is_some_and(|v| !v.is_empty())
}

fn debug(msg: &str) {
    if dev_debug() {
        eprintln!("[layer-overlay] {msg}");
    }
}

/// True when this session can show layer-shell overlays (probed once).
pub fn available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        if std::env::var_os("CODEXBAR_NO_LAYER_SHELL").is_some_and(|v| !v.is_empty() && v != "0")
            || std::env::var_os("WAYLAND_DISPLAY").is_none_or(|v| v.is_empty())
        {
            return false;
        }
        let ok = (|| {
            let conn = Connection::connect_to_env().ok()?;
            let (globals, queue) = registry_queue_init::<Overlay>(&conn).ok()?;
            LayerShell::bind(&globals, &queue.handle()).ok()
        })()
        .is_some();
        debug(&format!("zwlr_layer_shell_v1 available: {ok}"));
        ok
    })
}

/// Dev hook: `CODEXBAR_LAYER_OVERLAY_TEST=alert|confetti` shows that overlay once at startup.
pub fn dev_test() {
    match std::env::var("CODEXBAR_LAYER_OVERLAY_TEST").as_deref() {
        Ok("alert") => {
            let shown = show_alert(&Alert {
                title: "CodexBar · Codex".into(),
                message: "Session: 8% quota remaining.".into(),
            });
            debug(&format!("test alert started: {shown}"));
        }
        Ok("confetti") => {
            let shown = show_confetti("codex");
            debug(&format!("test confetti started: {shown}"));
        }
        _ => {}
    }
}

enum Kind {
    Alert(Alert),
    Confetti(String),
}

impl Kind {
    fn is_alert(&self) -> bool {
        matches!(self, Kind::Alert(_))
    }
}

/// Cancel flags of running overlays (`true` = alert); an entry is finished once its thread
/// dropped its own clone.
static ACTIVE: Mutex<Vec<(bool, Arc<AtomicBool>)>> = Mutex::new(Vec::new());

pub fn show_alert(alert: &Alert) -> bool {
    spawn(Kind::Alert(alert.clone()))
}

pub fn show_confetti(provider: &str) -> bool {
    spawn(Kind::Confetti(provider.to_string()))
}

pub fn dismiss_all() {
    if let Ok(mut active) = ACTIVE.lock() {
        for (_, flag) in active.drain(..) {
            flag.store(true, Ordering::Relaxed);
        }
    }
}

/// Starts an overlay thread. Returns `false` (nothing shown) if the layer path failed during
/// connect/bind/shm setup, so the caller can use the iced window instead.
fn spawn(kind: Kind) -> bool {
    if !available() {
        return false;
    }
    let is_alert = kind.is_alert();
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let thread_cancel = cancel.clone();
    let name = if is_alert {
        "layer-alert"
    } else {
        "layer-confetti"
    };
    let started = std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            if let Err(e) = run(kind, &thread_cancel, &tx) {
                debug(&format!("overlay failed: {e}"));
                let _ = tx.send(false);
            }
        })
        .is_ok();
    if !started || !rx.recv_timeout(SETUP_TIMEOUT).unwrap_or(false) {
        cancel.store(true, Ordering::Relaxed);
        return false;
    }
    if let Ok(mut active) = ACTIVE.lock() {
        // A new overlay replaces a running one of the same kind; finished threads are pruned.
        active.retain(|(a, flag)| {
            if *a == is_alert {
                flag.store(true, Ordering::Relaxed);
            }
            *a != is_alert && Arc::strong_count(flag) > 1
        });
        active.push((is_alert, cancel));
    }
    true
}

// ---- drawing (pure, no compositor) ----------------------------------------------------------

pub struct Text {
    fs: FontSystem,
    cache: SwashCache,
}

impl Text {
    pub fn new() -> Self {
        Self {
            fs: FontSystem::new(),
            cache: SwashCache::new(),
        }
    }

    fn buffer(
        &mut self,
        text: &str,
        size: f32,
        line: f32,
        width: Option<f32>,
        bold: bool,
    ) -> Buffer {
        let mut buf = Buffer::new(&mut self.fs, Metrics::new(size, line));
        buf.set_size(&mut self.fs, width, None);
        let mut attrs = Attrs::new().family(Family::SansSerif);
        if bold {
            attrs = attrs.weight(Weight::SEMIBOLD);
        }
        buf.set_text(&mut self.fs, text, attrs, Shaping::Advanced);
        buf.shape_until_scroll(&mut self.fs, true);
        buf
    }

    fn draw(&mut self, pm: &mut Pixmap, buf: &Buffer, ox: f32, oy: f32, rgba: [u8; 4]) {
        let color = cosmic_text::Color::rgba(rgba[0], rgba[1], rgba[2], rgba[3]);
        let mut paint = Paint::default();
        buf.draw(&mut self.fs, &mut self.cache, color, |x, y, w, h, c| {
            if c.a() == 0 {
                return;
            }
            if let Some(rect) = Rect::from_xywh(ox + x as f32, oy + y as f32, w as f32, h as f32) {
                paint.set_color_rgba8(c.r(), c.g(), c.b(), c.a());
                pm.fill_rect(rect, &paint, Transform::identity(), None);
            }
        });
    }
}

/// Logical height of the alert surface: card (padding + title row + wrapped message) plus the
/// transparent inset around it.
fn alert_height(text: &mut Text, alert: &Alert) -> f32 {
    let msg = text.buffer(
        &alert.message,
        MESSAGE_SIZE,
        MESSAGE_LINE,
        Some(ALERT_WIDTH - 2.0 * CARD_INSET - 2.0 * CARD_PAD_X),
        false,
    );
    let lines = msg.layout_runs().count().max(1) as f32;
    2.0 * CARD_INSET + 2.0 * CARD_PAD_Y + TITLE_LINE + 6.0 + lines * MESSAGE_LINE
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    let k = r * 0.552_284_8;
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

/// Renders the alert card at `scale` (surface is `ALERT_WIDTH x alert_height`, times scale).
pub fn render_alert(text: &mut Text, alert: &Alert, scale: f32) -> Option<Pixmap> {
    let height = alert_height(text, alert);
    let mut pm = Pixmap::new(
        (ALERT_WIDTH * scale).round() as u32,
        (height * scale).round() as u32,
    )?;
    let s = Transform::from_scale(scale, scale);
    let (cx, cy) = (CARD_INSET, CARD_INSET);
    let (cw, ch) = (ALERT_WIDTH - 2.0 * CARD_INSET, height - 2.0 * CARD_INSET);
    let mut paint = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    paint.set_color_rgba8(28, 28, 33, 240); // rgba(.11,.11,.13,.94)
    if let Some(path) = rounded_rect(cx, cy, cw, ch, 16.0) {
        pm.fill_path(&path, &paint, FillRule::Winding, s, None);
    }
    paint.set_color_rgba8(255, 255, 255, 26); // 10 % white border, 1 px
    if let Some(path) = rounded_rect(cx + 0.5, cy + 0.5, cw - 1.0, ch - 1.0, 15.5) {
        let stroke = Stroke {
            width: 1.0,
            ..Stroke::default()
        };
        pm.stroke_path(&path, &paint, &stroke, s, None);
    }
    let (tx, ty) = (cx + CARD_PAD_X, cy + CARD_PAD_Y);
    let bang = text.buffer("!", 20.0 * scale, TITLE_LINE * scale, None, false);
    text.draw(
        &mut pm,
        &bang,
        tx * scale,
        ty * scale,
        [0xff, 0x9f, 0x0a, 0xff],
    );
    let title = text.buffer(&alert.title, 17.0 * scale, TITLE_LINE * scale, None, true);
    text.draw(
        &mut pm,
        &title,
        (tx + 16.0) * scale,
        ty * scale,
        [255, 255, 255, 255],
    );
    let msg = text.buffer(
        &alert.message,
        MESSAGE_SIZE * scale,
        MESSAGE_LINE * scale,
        Some((cw - 2.0 * CARD_PAD_X) * scale),
        false,
    );
    text.draw(
        &mut pm,
        &msg,
        tx * scale,
        (ty + TITLE_LINE + 6.0) * scale,
        [255, 255, 255, 191], // 75 % white
    );
    Some(pm)
}

/// Same five-colour palette as the iced confetti: accent, lighter, darker, white, yellow.
pub fn confetti_palette(provider: &str) -> Vec<[u8; 3]> {
    let (r, g, b) = crate::ui::popover::accent_rgb(provider);
    let lift = |c: u8| (c as f32 + (255.0 - c as f32) * 0.4) as u8;
    let dim = |c: u8| (c as f32 * 0.7) as u8;
    vec![
        [r, g, b],
        [lift(r), lift(g), lift(b)],
        [dim(r), dim(g), dim(b)],
        [255, 255, 255],
        [0xff, 0xd6, 0x0a],
    ]
}

/// Clears `pm` and draws every particle at `t` seconds.
pub fn render_confetti(
    pm: &mut Pixmap,
    particles: &[overlay::Particle],
    palette: &[[u8; 3]],
    t: f32,
) {
    pm.fill(tiny_skia::Color::TRANSPARENT);
    let mut paint = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    for p in particles {
        let (x, y, rot, alpha) = overlay::pose(p, t);
        if alpha <= 0.0 {
            continue;
        }
        let c = palette[p.color % palette.len()];
        paint.set_color_rgba8(c[0], c[1], c[2], (alpha * 255.0) as u8);
        if let Some(rect) = Rect::from_xywh(-p.w / 2.0, -p.h / 2.0, p.w, p.h) {
            let xf = Transform::from_translate(x, y).pre_rotate(rot.to_degrees());
            pm.fill_rect(rect, &paint, xf, None);
        }
    }
}

// ---- wayland --------------------------------------------------------------------------------

enum Mode {
    Alert {
        alert: Alert,
        text: Box<Text>,
    },
    Confetti {
        particles: Vec<overlay::Particle>,
        palette: Vec<[u8; 3]>,
        canvas: Option<Pixmap>,
    },
}

struct Overlay {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    layer: LayerSurface,
    pointer: Option<wl_pointer::WlPointer>,
    mode: Mode,
    size: (u32, u32),
    scale: i32,
    configured: bool,
    frames: u32,
    started: Instant,
    done: bool,
}

fn run(kind: Kind, cancel: &AtomicBool, ack: &mpsc::Sender<bool>) -> Result<(), String> {
    let conn = Connection::connect_to_env().map_err(|e| format!("connect: {e}"))?;
    let (globals, mut queue) =
        registry_queue_init::<Overlay>(&conn).map_err(|e| format!("registry: {e}"))?;
    let qh = queue.handle();
    let compositor =
        CompositorState::bind(&globals, &qh).map_err(|e| format!("compositor: {e}"))?;
    let layer_shell = LayerShell::bind(&globals, &qh).map_err(|e| format!("layer shell: {e}"))?;
    let shm = Shm::bind(&globals, &qh).map_err(|e| format!("shm: {e}"))?;
    let pool = SlotPool::new(4096, &shm).map_err(|e| format!("shm pool: {e}"))?;
    let _ = ack.send(true);

    let surface = compositor.create_surface(&qh);
    let is_alert = kind.is_alert();
    let (mut mode, namespace) = match kind {
        Kind::Alert(alert) => (
            Mode::Alert {
                alert,
                text: Box::new(Text::new()),
            },
            "codexbar-alert",
        ),
        Kind::Confetti(provider) => (
            Mode::Confetti {
                particles: Vec::new(),
                palette: confetti_palette(&provider),
                canvas: None,
            },
            "codexbar-confetti",
        ),
    };
    let layer =
        layer_shell.create_layer_surface(&qh, surface, Layer::Overlay, Some(namespace), None);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    if let Mode::Alert { alert, text } = &mut mode {
        layer.set_anchor(Anchor::TOP);
        layer.set_margin(ALERT_MARGIN_TOP, 0, 0, 0);
        layer.set_size(ALERT_WIDTH as u32, alert_height(text, alert).ceil() as u32);
    } else {
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_exclusive_zone(-1);
        layer.set_size(0, 0);
        // Empty input region: every click goes to the window underneath.
        let region = Region::new(&compositor).map_err(|e| format!("region: {e}"))?;
        layer
            .wl_surface()
            .set_input_region(Some(region.wl_region()));
    }
    layer.commit();

    let mut state = Overlay {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm,
        pool,
        layer,
        pointer: None,
        mode,
        size: (ALERT_WIDTH as u32, 150),
        scale: 1,
        configured: false,
        frames: 0,
        started: Instant::now(),
        done: false,
    };
    let lifetime = if is_alert {
        ALERT_SECS
    } else {
        CONFETTI_SECS + 0.5
    };
    loop {
        queue
            .dispatch_pending(&mut state)
            .map_err(|e| format!("dispatch: {e}"))?;
        if state.done
            || cancel.load(Ordering::Relaxed)
            || state.started.elapsed().as_secs_f32() > lifetime
        {
            break;
        }
        queue.flush().map_err(|e| format!("flush: {e}"))?;
        let Some(guard) = queue.prepare_read() else {
            continue;
        };
        let mut pfd = libc::pollfd {
            fd: guard.connection_fd().as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: `pfd` is a valid pollfd for the duration of the call.
        let ready = unsafe { libc::poll(&mut pfd, 1, 50) } > 0;
        if ready {
            if let Err(e) = guard.read() {
                let would_block = matches!(
                    &e,
                    wayland_client::backend::WaylandError::Io(io)
                        if io.kind() == std::io::ErrorKind::WouldBlock
                );
                if !would_block {
                    return Err(format!("read: {e}"));
                }
            }
        }
    }
    debug(&format!(
        "{namespace} closed after {} frame(s)",
        state.frames
    ));
    Ok(())
}

impl Overlay {
    fn draw(&mut self, qh: &QueueHandle<Self>) {
        let t = self.started.elapsed().as_secs_f32();
        let (width, height) = self.size;
        let scale = self.scale.max(1);
        let alert = matches!(self.mode, Mode::Alert { .. });
        let rendered = match &mut self.mode {
            Mode::Alert { alert, text } => render_alert(text, alert, scale as f32),
            Mode::Confetti {
                particles,
                palette,
                canvas,
            } => {
                if particles.is_empty() {
                    *particles = overlay::particles(width as f32, height as f32, palette.len());
                }
                if canvas
                    .as_ref()
                    .is_none_or(|c| (c.width(), c.height()) != (width, height))
                {
                    *canvas = Pixmap::new(width, height);
                }
                // Moved out (and put back after the copy) to avoid cloning a full-screen pixmap.
                canvas.take().map(|mut c| {
                    render_confetti(&mut c, particles, palette, t);
                    c
                })
            }
        };
        let Some(pm) = rendered else {
            return;
        };
        let (pw, ph) = (pm.width() as i32, pm.height() as i32);
        let Ok((buffer, canvas)) =
            self.pool
                .create_buffer(pw, ph, pw * 4, wl_shm::Format::Argb8888)
        else {
            debug("shm buffer allocation failed");
            self.done = true;
            return;
        };
        // tiny-skia is premultiplied RGBA; wl_shm Argb8888 is premultiplied BGRA in memory.
        let (dst4, _) = canvas.as_chunks_mut::<4>();
        for (dst, src) in dst4.iter_mut().zip(pm.pixels()) {
            *dst = [src.blue(), src.green(), src.red(), src.alpha()];
        }
        if let Mode::Confetti { canvas, .. } = &mut self.mode {
            *canvas = Some(pm);
        }
        let surface = self.layer.wl_surface();
        surface.set_buffer_scale(if alert { scale } else { 1 });
        surface.damage_buffer(0, 0, pw, ph);
        if !alert {
            surface.frame(qh, surface.clone());
        }
        if buffer.attach_to(surface).is_err() {
            self.done = true;
            return;
        }
        self.layer.commit();
        self.frames += 1;
        if self.frames == 1 {
            debug(&format!("first frame rendered: {pw}x{ph} scale {scale}"));
        }
    }
}

impl CompositorHandler for Overlay {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        new_factor: i32,
    ) {
        self.scale = new_factor.max(1);
        // Only the alert is scale-aware; confetti is drawn at 1x and upscaled by the compositor.
        if self.configured && matches!(self.mode, Mode::Alert { .. }) {
            self.draw(qh);
        }
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        if matches!(self.mode, Mode::Confetti { .. }) {
            if self.started.elapsed().as_secs_f32() >= CONFETTI_SECS {
                self.done = true;
            } else {
                self.draw(qh);
            }
        }
    }

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for Overlay {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for Overlay {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.done = true;
    }

    fn configure(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        let (w, h) = configure.new_size;
        if w > 0 && h > 0 {
            self.size = (w, h);
        } else if matches!(self.mode, Mode::Confetti { .. }) {
            self.size = (1280, 720);
        }
        debug(&format!(
            "layer surface configured: {}x{}",
            self.size.0, self.size.1
        ));
        if !self.configured {
            self.configured = true;
            self.draw(qh);
        }
    }
}

impl SeatHandler for Overlay {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        // Only the alert takes clicks; the confetti has an empty input region.
        if capability == Capability::Pointer
            && self.pointer.is_none()
            && matches!(self.mode, Mode::Alert { .. })
        {
            self.pointer = self.seat_state.get_pointer(qh, &seat).ok();
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer {
            if let Some(p) = self.pointer.take() {
                p.release();
            }
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for Overlay {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            if &event.surface == self.layer.wl_surface()
                && matches!(event.kind, PointerEventKind::Press { .. })
            {
                debug("alert dismissed by click");
                self.done = true;
            }
        }
    }
}

impl ShmHandler for Overlay {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_compositor!(Overlay);
delegate_output!(Overlay);
delegate_shm!(Overlay);
delegate_seat!(Overlay);
delegate_pointer!(Overlay);
delegate_layer!(Overlay);
delegate_registry!(Overlay);

impl ProvidesRegistryState for Overlay {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opaque_pixels(pm: &Pixmap) -> usize {
        pm.pixels().iter().filter(|p| p.alpha() > 0).count()
    }

    #[test]
    fn alert_frame_has_card_pixels_and_expected_size() {
        let mut text = Text::new();
        let alert = Alert {
            title: "CodexBar · Codex".into(),
            message: "Session: 8% quota remaining.".into(),
        };
        let h = alert_height(&mut text, &alert);
        assert!(h > 60.0 && h < 400.0);
        let pm = render_alert(&mut text, &alert, 1.0).expect("pixmap");
        assert_eq!((pm.width(), pm.height()), (480, h.round() as u32));
        assert!(opaque_pixels(&pm) > 10_000);
        // Corner outside the rounded card stays transparent; the card centre does not.
        assert_eq!(pm.pixel(0, 0).unwrap().alpha(), 0);
        assert!(pm.pixel(240, pm.height() / 2).unwrap().alpha() > 200);
        let hi = render_alert(&mut text, &alert, 2.0).expect("pixmap");
        assert_eq!((hi.width(), hi.height()), (960, (h * 2.0).round() as u32));
    }

    #[test]
    fn confetti_frame_draws_particles_then_fades_out() {
        let particles = overlay::particles(400.0, 300.0, 5);
        let palette = confetti_palette("codex");
        let mut pm = Pixmap::new(400, 300).unwrap();
        render_confetti(&mut pm, &particles, &palette, 0.8);
        assert_eq!((pm.width(), pm.height()), (400, 300));
        assert!(opaque_pixels(&pm) > 100);
        render_confetti(&mut pm, &particles, &palette, CONFETTI_SECS);
        assert_eq!(opaque_pixels(&pm), 0);
    }
}
