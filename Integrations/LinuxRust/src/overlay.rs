//! Quota warning overlay + limit-reset confetti (mac `QuotaWarningAlertOverlayController`,
//! `ScreenConfettiOverlayController`). Pure logic only: which transitions fire which overlay
//! (reusing `notify::transition_events`) and the deterministic confetti particle model. The
//! windows live in `main.rs`, the drawing in `ui/overlay_view.rs`.
//!
//! mac conditions: the on-screen alert rides on a quota-warning notification (so it needs
//! notifications on), confetti fires on an observed limit reset that lands at <= 1 % used and
//! is gated per series (session = primary window, weekly = secondary) independently of
//! notifications. Anything else (status notices, scoped/extra windows, unknown keys) is silent.

use crate::extras::Extras;
use crate::notify::Notice;

/// Alert card lifetime (mac `overlayLifetime`).
pub const ALERT_SECS: f32 = 4.5;
/// Confetti run time (mac particles live up to 8 s; a few seconds is enough here).
pub const CONFETTI_SECS: f32 = 5.0;
/// mac `limitResetThreshold`: a reset is celebrated when used <= 1 %.
const RESET_MAX_USED: i64 = 1;

/// Dev-only: `CODEXBAR_FORCE_OVERLAY=1` shows both overlays without waiting for a quota event,
/// never auto-dismisses them and freezes the confetti clock (screenshot captures).
pub fn forced() -> bool {
    std::env::var_os("CODEXBAR_FORCE_OVERLAY").is_some_and(|v| !v.is_empty() && v != "0")
}

#[derive(Debug, Clone, PartialEq)]
pub struct Alert {
    pub title: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfettiSpec {
    pub provider: String,
}

#[derive(Debug, Default, PartialEq)]
pub struct Triggers {
    pub alert: Option<Alert>,
    pub confetti: Option<ConfettiSpec>,
}

pub fn triggers(events: &[Notice], notifications: bool, extras: &Extras) -> Triggers {
    let mut out = Triggers::default();
    for ev in events {
        match ev.kind {
            "low" if notifications && extras.quota_warning_overlay && out.alert.is_none() => {
                // Only real quota windows alert; a notice without a window never does.
                if !ev.window_key.is_empty() {
                    out.alert = Some(Alert {
                        title: format!(
                            "CodexBar · {}",
                            crate::descriptors::display_name(&ev.provider)
                        ),
                        message: ev.message.clone(),
                    });
                }
            }
            "reset" if out.confetti.is_none() && 100 - ev.remaining <= RESET_MAX_USED => {
                let enabled = match ev.window_key.as_str() {
                    "primary" => extras.confetti_on_session_reset,
                    "secondary" => extras.confetti_on_weekly_reset,
                    _ => false,
                };
                if enabled {
                    out.confetti = Some(ConfettiSpec {
                        provider: ev.provider.clone(),
                    });
                }
            }
            _ => {}
        }
    }
    out
}

// ---- confetti model -------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Particle {
    pub x0: f32,
    pub y0: f32,
    pub vx: f32,
    pub vy: f32,
    pub rot0: f32,
    pub spin: f32,
    pub w: f32,
    pub h: f32,
    pub color: usize,
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f32 {
        // xorshift64*; deterministic so a given size always yields the same burst.
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let v = self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40;
        v as f32 / (1u64 << 24) as f32
    }
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next()
    }
}

const DRAG: f32 = 0.9;
const GRAVITY: f32 = 780.0;

/// Fireworks-style bursts from three points along the bottom edge, fanning upward.
pub fn particles(width: f32, height: f32, palette_len: usize) -> Vec<Particle> {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let palette_len = palette_len.max(1);
    let mut out = Vec::with_capacity(150);
    for (i, ox) in [0.2_f32, 0.5, 0.8].iter().enumerate() {
        let speed_scale = height.max(200.0) / 700.0;
        for _ in 0..50 {
            let angle = (-90.0 + rng.range(-38.0, 38.0)).to_radians();
            let speed = rng.range(420.0, 1000.0) * speed_scale;
            let bar = rng.next() < 0.6;
            out.push(Particle {
                x0: width * ox,
                y0: height + 6.0,
                vx: angle.cos() * speed,
                vy: angle.sin() * speed,
                rot0: rng.range(0.0, std::f32::consts::TAU),
                spin: rng.range(-9.0, 9.0),
                w: if bar { 8.0 } else { 9.0 },
                h: if bar { 16.0 } else { 9.0 },
                color: (i + (rng.next() * palette_len as f32) as usize) % palette_len,
            });
        }
    }
    out
}

/// (x, y, rotation, alpha) at `t` seconds; alpha fades over the last second.
pub fn pose(p: &Particle, t: f32) -> (f32, f32, f32, f32) {
    let decay = (1.0 - (-DRAG * t).exp()) / DRAG;
    let x = p.x0 + p.vx * decay;
    let y = p.y0 + (p.vy - GRAVITY / DRAG) * decay + (GRAVITY / DRAG) * t;
    let alpha = ((CONFETTI_SECS - t) / 1.0).clamp(0.0, 1.0);
    (x, y, p.rot0 + p.spin * t, alpha)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notice(kind: &'static str, key: &str, remaining: i64) -> Notice {
        Notice {
            kind,
            provider: "codex".into(),
            message: "msg".into(),
            window_key: key.into(),
            remaining,
        }
    }
    fn all_on() -> Extras {
        Extras {
            quota_warning_overlay: true,
            confetti_on_session_reset: true,
            confetti_on_weekly_reset: true,
            ..Default::default()
        }
    }

    #[test]
    fn defaults_never_fire() {
        let evs = [notice("low", "primary", 5), notice("reset", "primary", 100)];
        assert_eq!(
            triggers(&evs, true, &Extras::default()),
            Triggers::default()
        );
    }

    #[test]
    fn low_quota_fires_alert_only_with_notifications_and_toggle() {
        let evs = [notice("low", "primary", 8)];
        let t = triggers(&evs, true, &all_on());
        let alert = t.alert.expect("alert");
        assert_eq!(alert.message, "msg");
        assert!(t.confetti.is_none());
        assert!(triggers(&evs, false, &all_on()).alert.is_none());
        let off = Extras {
            quota_warning_overlay: false,
            ..all_on()
        };
        assert!(triggers(&evs, true, &off).alert.is_none());
    }

    #[test]
    fn reset_fires_confetti_per_series_and_ignores_notifications_flag() {
        let session = [notice("reset", "primary", 100)];
        assert!(triggers(&session, false, &all_on()).confetti.is_some());
        let only_weekly = Extras {
            confetti_on_session_reset: false,
            ..all_on()
        };
        assert!(triggers(&session, true, &only_weekly).confetti.is_none());
        let weekly = [notice("reset", "secondary", 99)];
        assert!(triggers(&weekly, true, &only_weekly).confetti.is_some());
        assert!(triggers(
            &weekly,
            true,
            &Extras {
                confetti_on_weekly_reset: false,
                ..all_on()
            }
        )
        .confetti
        .is_none());
    }

    #[test]
    fn partial_recovery_does_not_celebrate() {
        // A boundary shift that leaves 60 % remaining is not a fresh window (>1 % used).
        let evs = [notice("reset", "primary", 60)];
        assert!(triggers(&evs, true, &all_on()).confetti.is_none());
    }

    #[test]
    fn unknown_values_never_fire() {
        let evs = [
            notice("low", "", 5),              // no window
            notice("status", "primary", 100),  // status notice
            notice("mystery", "primary", 100), // unknown kind
            notice("reset", "extra:foo", 100), // scoped/extra window: not a series
            notice("reset", "tertiary", 100),  // unknown key
            notice("reset", "", 100),
        ];
        assert_eq!(triggers(&evs, true, &all_on()), Triggers::default());
        assert_eq!(triggers(&[], true, &all_on()), Triggers::default());
    }

    #[test]
    fn end_to_end_from_entries() {
        use crate::model::{Entry, QuotaWindow};
        let entry = |remaining: i64, resets: &str| Entry {
            provider: "codex".into(),
            account_label: String::new(),
            plan: String::new(),
            status: String::new(),
            status_level: "none".into(),
            error: String::new(),
            updated_at: String::new(),
            credits: None,
            windows: vec![QuotaWindow {
                key: "primary".into(),
                label: "Session".into(),
                minutes: 300,
                remaining,
                resets_at: resets.into(),
                reset_text: String::new(),
                pace: String::new(),
                pace_expected: 0.0,
                pace_delta: 0.0,
                scoped: false,
            }],
            details: vec![],
            extra_usage_text: String::new(),
            reset_credits_available: 0,
            reset_credits_next_expiry: String::new(),
        };
        let low = crate::notify::transition_events(
            &[entry(50, "2026-01-01T00:00:00Z")],
            &[entry(8, "2026-01-01T00:00:00Z")],
            10,
        );
        assert!(triggers(&low, true, &all_on()).alert.is_some());
        let reset = crate::notify::transition_events(
            &[entry(0, "2026-01-01T00:00:00Z")],
            &[entry(100, "2026-01-01T05:00:00Z")],
            10,
        );
        assert!(triggers(&reset, true, &all_on()).confetti.is_some());
        // First load / no change: nothing.
        let none = crate::notify::transition_events(&[], &[entry(5, "x")], 10);
        assert_eq!(triggers(&none, true, &all_on()), Triggers::default());
    }

    #[test]
    fn confetti_model_is_deterministic_and_bounded() {
        let a = particles(900.0, 700.0, 5);
        assert_eq!(a, particles(900.0, 700.0, 5));
        assert_eq!(a.len(), 150);
        assert!(a.iter().all(|p| p.color < 5 && p.vy < 0.0));
        let (x, y, _, alpha) = pose(&a[0], 0.0);
        assert!((x - a[0].x0).abs() < 1e-3 && (y - a[0].y0).abs() < 1e-3);
        assert_eq!(alpha, 1.0);
        // Rises first, then gravity pulls it back down.
        assert!(pose(&a[0], 0.5).1 < a[0].y0);
        assert!(pose(&a[0], 4.0).1 > pose(&a[0], 1.0).1);
        assert_eq!(pose(&a[0], CONFETTI_SECS).3, 0.0);
        assert!(a.iter().all(|p| {
            let (x, y, r, _) = pose(p, 2.5);
            x.is_finite() && y.is_finite() && r.is_finite()
        }));
        assert_eq!(particles(10.0, 10.0, 0).len(), 150);
    }
}
