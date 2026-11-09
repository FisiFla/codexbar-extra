//! IPC snapshot JSON + Unix-socket server, byte-compatible with the Qt app's
//! `DesktopController::snapshot()` / `listen()` (schemaVersion 1, newline JSON).
//! Bar text ports `Integrations/Linux/Shared/Usage.js` `barSegments`.

use crate::model::{Entry, QuotaWindow};
use crate::settings::Settings;
use crate::state::{IpcCommand, Shared};
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock};

// ---------------------------------------------------------------------------
// SHA-256 (no extra crate): Qt hashes provider-controlled `extra:` window
// keys with a per-process salt so they stay opaque yet stable over IPC.
// ---------------------------------------------------------------------------

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[4 * i],
                chunk[4 * i + 1],
                chunk[4 * i + 2],
                chunk[4 * i + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(SHA256_K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        h = [
            h[0].wrapping_add(a),
            h[1].wrapping_add(b),
            h[2].wrapping_add(c),
            h[3].wrapping_add(d),
            h[4].wrapping_add(e),
            h[5].wrapping_add(f),
            h[6].wrapping_add(g),
            h[7].wrapping_add(hh),
        ];
    }
    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[4 * i..4 * i + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

fn hex_digest(data: &[u8]) -> String {
    sha256(data).iter().map(|b| format!("{b:02x}")).collect()
}

fn extra_key_salt() -> &'static [u8; 16] {
    static SALT: OnceLock<[u8; 16]> = OnceLock::new();
    SALT.get_or_init(|| {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let mut seed = Vec::with_capacity(24);
        seed.extend_from_slice(&std::process::id().to_be_bytes());
        seed.extend_from_slice(&nanos.to_be_bytes());
        seed.extend_from_slice(&(seed.as_ptr() as usize).to_be_bytes());
        let digest = sha256(&seed);
        let mut salt = [0u8; 16];
        salt.copy_from_slice(&digest[..16]);
        salt
    })
}

fn hashed_extra_key(provider: &str, key: &str) -> String {
    let mut input = extra_key_salt().to_vec();
    input.extend_from_slice(format!("{provider}/{key}").as_bytes());
    format!("extra:{}", hex_digest(&input))
}

// ---------------------------------------------------------------------------
// resetText (port of Usage.js resetText/resetLabel)
// ---------------------------------------------------------------------------

fn parse_timestamp_ms(value: &str) -> Option<i64> {
    if value.is_empty() {
        return None;
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(value) {
        return Some(dt.timestamp_millis());
    }
    // JS `new Date(...)` also accepts naive local strings; try common shapes.
    for fmt in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S",
    ] {
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(value, fmt) {
            return Some(dt.and_utc().timestamp_millis());
        }
    }
    None
}

fn reset_label(ts_ms: i64, now_ms: i64) -> String {
    let minutes = ((ts_ms - now_ms) as f64 / 60_000.0).ceil() as i64;
    if minutes <= 0 {
        "Reset due · refresh to update".into()
    } else if minutes < 60 {
        format!("Resets in {minutes}m")
    } else if minutes < 1440 {
        format!("Resets in {}h {}m", minutes / 60, minutes % 60)
    } else {
        format!("Resets in {}d {}h", minutes / 1440, (minutes % 1440) / 60)
    }
}

/// Usage.js `resetText(timestamp, now, mode)` with an explicit clock.
pub fn reset_text_at(timestamp: &str, now_ms: i64, mode: &str) -> String {
    let Some(ts) = parse_timestamp_ms(timestamp) else {
        return "Reset time unavailable".into();
    };
    let absolute = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ts)
        .map(|dt| {
            dt.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_default();
    match mode {
        "absolute" => format!("Resets {absolute}"),
        "both" => format!("{} · {}", reset_label(ts, now_ms), absolute),
        _ => reset_label(ts, now_ms),
    }
}

// ---------------------------------------------------------------------------
// Bar segments (port of Usage.js laneSegments/barSegments)
// ---------------------------------------------------------------------------

fn quota_value(remaining: i64, used: bool) -> i64 {
    if used {
        100 - remaining
    } else {
        remaining
    }
}

fn has_cadence(w: &QuotaWindow) -> bool {
    w.minutes > 0
}

/// First index (stable, like the JS stable sort) with the least remaining.
fn tightest(windows: &[QuotaWindow], candidates: &[usize]) -> Option<usize> {
    candidates
        .iter()
        .fold(None, |best: Option<usize>, &i| match best {
            None => Some(i),
            Some(b) if windows[i].remaining < windows[b].remaining => Some(i),
            other => other,
        })
}

fn lane_of_cadence(
    windows: &[QuotaWindow],
    matches: impl Fn(&QuotaWindow) -> bool,
) -> Option<usize> {
    let summarised: Vec<usize> = windows
        .iter()
        .enumerate()
        .filter(|(_, w)| matches(w) && w.key.contains("quota-summary"))
        .map(|(i, _)| i)
        .collect();
    if !summarised.is_empty() {
        return tightest(windows, &summarised);
    }
    let general: Vec<usize> = windows
        .iter()
        .enumerate()
        .filter(|(_, w)| !w.scoped && matches(w))
        .map(|(i, _)| i)
        .collect();
    if !general.is_empty() {
        return tightest(windows, &general);
    }
    let scoped: Vec<usize> = windows
        .iter()
        .enumerate()
        .filter(|(_, w)| w.scoped && matches(w))
        .map(|(i, _)| i)
        .collect();
    tightest(windows, &scoped)
}

fn session_window(windows: &[QuotaWindow]) -> Option<usize> {
    lane_of_cadence(windows, |w| (60..=720).contains(&w.minutes))
}

fn weekly_window(windows: &[QuotaWindow]) -> Option<usize> {
    lane_of_cadence(windows, |w| w.minutes == 10080)
}

fn lane_label(minutes: i64) -> String {
    if minutes <= 0 {
        String::new()
    } else if minutes >= 1440 {
        format!("{}D", (minutes as f64 / 1440.0).round() as i64)
    } else if minutes % 60 == 0 {
        format!("{}H", minutes / 60)
    } else {
        format!("{minutes}M")
    }
}

/// Pace is unavailable (None) when the model carries the NaN sentinel
/// (Usage.js `null`); never reported as 0.
fn pace_delta(w: &QuotaWindow) -> Option<f64> {
    if w.pace_delta.is_finite() {
        Some(w.pace_delta)
    } else {
        None
    }
}

fn pace_expected(w: &QuotaWindow) -> Option<f64> {
    if w.pace_expected.is_finite() {
        Some(w.pace_expected)
    } else {
        None
    }
}

fn pace_delta_text(delta: Option<f64>) -> String {
    let Some(delta) = delta else {
        return String::new();
    };
    if !delta.is_finite() {
        return String::new();
    }
    let value = delta.abs().round() as i64;
    if value == 0 {
        "0%".into()
    } else if delta >= 0.0 {
        format!("+{value}%")
    } else {
        format!("-{value}%")
    }
}

fn promoted_fallback(windows: &[QuotaWindow]) -> Option<usize> {
    if windows.iter().any(|w| !w.scoped) {
        return None;
    }
    let candidates: Vec<usize> = windows
        .iter()
        .enumerate()
        .filter(|(_, w)| w.scoped && !has_cadence(w) && w.key.contains("compact-fallback"))
        .map(|(i, _)| i)
        .collect();
    tightest(windows, &candidates)
}

fn tightest_general(windows: &[QuotaWindow]) -> Option<usize> {
    let fallback = promoted_fallback(windows);
    let candidates: Vec<usize> = windows
        .iter()
        .enumerate()
        .filter(|(i, w)| !w.scoped || fallback == Some(*i))
        .map(|(i, _)| i)
        .collect();
    tightest(windows, &candidates)
}

fn other_cadences(windows: &[QuotaWindow], shown: &[Option<usize>; 2]) -> Vec<usize> {
    let covered: Vec<usize> = shown.iter().flatten().copied().collect();
    let claimed: HashSet<i64> = windows
        .iter()
        .filter(|w| !w.scoped && has_cadence(w))
        .map(|w| w.minutes)
        .collect();
    let fallback = promoted_fallback(windows);
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<usize> = windows
        .iter()
        .enumerate()
        .filter(|(i, w)| {
            if w.scoped && fallback != Some(*i) && (!has_cadence(w) || claimed.contains(&w.minutes))
            {
                return false;
            }
            let key = if has_cadence(w) {
                w.minutes.to_string()
            } else {
                format!("named:{}", w.label)
            };
            if seen.contains(&key)
                || covered
                    .iter()
                    .any(|&o| o == *i || (has_cadence(w) && windows[o].minutes == w.minutes))
            {
                return false;
            }
            seen.insert(key);
            true
        })
        .map(|(i, _)| i)
        .collect();
    out.sort_by_key(|&i| {
        if has_cadence(&windows[i]) {
            windows[i].minutes
        } else {
            i64::MAX
        }
    });
    out
}

fn binding_scope(
    item: usize,
    session: Option<usize>,
    weekly: Option<usize>,
    windows: &[QuotaWindow],
) -> bool {
    let w = &windows[item];
    let mut general = if w.minutes == 10080 {
        weekly
    } else if (60..=720).contains(&w.minutes) {
        session
    } else {
        None
    };
    if general.is_none() || general == Some(item) {
        general = tightest_general(windows);
    }
    match general {
        None => true,
        Some(g) => w.remaining < windows[g].remaining,
    }
}

fn lane_segments(entry: &Entry, used: bool, settings: &Settings) -> Vec<String> {
    let windows = &entry.windows;
    let session = session_window(windows);
    let weekly = weekly_window(windows);
    let mut segments: Vec<String> = Vec::new();
    let mut rendered: Vec<usize> = Vec::new();
    if !settings.show_bar_detail {
        if let Some(first) = windows.first() {
            segments.push(format!("{}%", quota_value(first.remaining, used)));
            rendered.push(0);
        }
    } else {
        if let Some(s) = session {
            segments.push(format!(
                "{} {}%",
                lane_label(windows[s].minutes),
                quota_value(windows[s].remaining, used)
            ));
        }
        if let Some(w) = weekly {
            segments.push(format!(
                "{} {}%",
                lane_label(windows[w].minutes),
                quota_value(windows[w].remaining, used)
            ));
        }
        let extra = other_cadences(windows, &[session, weekly]);
        for &i in &extra {
            let label = lane_label(windows[i].minutes);
            let label = if label.is_empty() {
                windows[i].label.clone()
            } else {
                label
            };
            segments.push(format!(
                "{label} {}%",
                quota_value(windows[i].remaining, used)
            ));
        }
        rendered = [session, weekly]
            .into_iter()
            .flatten()
            .chain(extra)
            .collect();
    }
    for (i, w) in windows.iter().enumerate() {
        if !settings.show_scoped_caps || !w.scoped || rendered.contains(&i) {
            continue;
        }
        let model_cap = match entry.provider.as_str() {
            "claude" => w.key.starts_with("extra:claude-weekly-scoped-"),
            "codex" => w.key.starts_with("extra:codex-"),
            _ => false,
        };
        if !model_cap && !binding_scope(i, session, weekly, windows) {
            continue;
        }
        let label = strip_only_suffix(&w.label);
        segments.push(format!("{label} {}%", quota_value(w.remaining, used)));
    }
    if settings.show_bar_detail && settings.show_pace {
        if let Some(w) = weekly {
            let text = pace_delta_text(pace_delta(&windows[w]));
            if !text.is_empty() {
                segments.push(text);
            }
        }
    }
    segments
}

/// Usage.js: `label.replace(/\s+only$/i, "")`.
fn strip_only_suffix(label: &str) -> String {
    let trimmed = label.trim_end();
    if let Some(tail) = trimmed.get(trimmed.len().saturating_sub(4)..) {
        if tail.eq_ignore_ascii_case("only") {
            let head = &trimmed[..trimmed.len() - 4];
            if head.ends_with(|c: char| c.is_whitespace()) {
                return head.trim_end().to_string();
            }
        }
    }
    label.to_string()
}

fn bar_tag(provider: &str) -> String {
    match provider {
        "codex" => "CX".into(),
        "claude" => "CL".into(),
        other => other.to_string(),
    }
}

fn bar_entries(entries: &[Entry], settings: &Settings) -> Value {
    let used = settings.quota_display == "used";
    let limit = settings.bar_providers as usize;
    let shown: &[Entry] = if limit > 0 {
        &entries[..limit.min(entries.len())]
    } else {
        entries
    };
    Value::Array(
        shown
            .iter()
            .map(|entry| {
                let segments = lane_segments(entry, used, settings);
                let text = if segments.is_empty() {
                    "—".to_string()
                } else {
                    segments.join(" · ")
                };
                json!({ "provider": entry.provider, "tag": bar_tag(&entry.provider), "text": text })
            })
            .collect(),
    )
}

// ---------------------------------------------------------------------------
// Snapshot JSON (DesktopController::snapshot, schemaVersion 1)
// ---------------------------------------------------------------------------

fn num_or_null(value: Option<f64>) -> Value {
    match value {
        Some(v) => serde_json::Number::from_f64(v)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        None => Value::Null,
    }
}

fn window_json(w: &QuotaWindow, provider: &str, settings: &Settings, now_ms: i64) -> Value {
    let used = settings.quota_display == "used";
    let key = if w.key.starts_with("extra:") {
        hashed_extra_key(provider, &w.key)
    } else {
        w.key.clone()
    };
    // Alphabetical key order, as emitted by QJsonObject.
    let mut map = Map::new();
    map.insert(
        "displaySuffix".into(),
        json!(if used { "used" } else { "left" }),
    );
    map.insert("displayValue".into(), json!(quota_value(w.remaining, used)));
    map.insert("key".into(), json!(key));
    map.insert("label".into(), json!(w.label));
    map.insert("minutes".into(), json!(w.minutes));
    map.insert("pace".into(), json!(w.pace));
    map.insert("paceDelta".into(), num_or_null(pace_delta(w)));
    map.insert("paceExpected".into(), num_or_null(pace_expected(w)));
    map.insert("remaining".into(), json!(w.remaining));
    map.insert(
        "resetText".into(),
        json!(reset_text_at(&w.resets_at, now_ms, &settings.reset_display)),
    );
    map.insert("resetsAt".into(), json!(w.resets_at));
    if w.scoped {
        map.insert("scoped".into(), json!(true));
    }
    map.insert(
        "warning".into(),
        json!(settings.warning_colors && w.remaining <= settings.notify_threshold as i64),
    );
    Value::Object(map)
}

/// Build the snapshot object. Never carries `account_label`/email.
pub fn snapshot(s: &Shared) -> Value {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let settings = &s.settings;
    let entries: Vec<Value> = s
        .entries
        .iter()
        .map(|entry| {
            let windows: Vec<Value> = entry
                .windows
                .iter()
                .map(|w| window_json(w, &entry.provider, settings, now_ms))
                .collect();
            let mut map = Map::new();
            map.insert("credits".into(), num_or_null(entry.credits));
            map.insert("error".into(), json!(entry.error));
            map.insert("extraUsage".into(), Value::Null);
            map.insert("plan".into(), json!(entry.plan));
            map.insert("provider".into(), json!(entry.provider));
            // Model sentinel: available < 0 means unreported -> null (Qt/JS).
            let reset_credits = if entry.reset_credits_available < 0 {
                Value::Null
            } else {
                json!({
                    "available": entry.reset_credits_available,
                    "nextExpiresAt": entry.reset_credits_next_expiry,
                })
            };
            map.insert("resetCredits".into(), reset_credits);
            map.insert("status".into(), json!(entry.status));
            map.insert("statusLevel".into(), json!(entry.status_level));
            map.insert("updatedAt".into(), json!(entry.updated_at));
            map.insert("windows".into(), Value::Array(windows));
            Value::Object(map)
        })
        .collect();
    let spending: Vec<Value> = s
        .spending
        .iter()
        .map(|c| {
            // NaN sentinel -> null, never 0 for unknown spend.
            let mut row = Map::new();
            row.insert("provider".into(), json!(c.provider));
            row.insert("today".into(), num_or_null(Some(c.today)));
            row.insert("month".into(), num_or_null(Some(c.month)));
            row.insert("tokens".into(), json!(c.tokens));
            row.insert("provenance".into(), json!(c.provenance));
            row.insert("error".into(), json!(c.error));
            Value::Object(row)
        })
        .collect();
    let mut map = Map::new();
    map.insert("barEntries".into(), bar_entries(&s.entries, settings));
    map.insert("busy".into(), json!(s.busy));
    map.insert("costBusy".into(), json!(s.cost_busy));
    map.insert("costError".into(), json!(s.cost_error));
    map.insert("costProviders".into(), json!(s.spending.len()));
    map.insert("entries".into(), Value::Array(entries));
    map.insert("error".into(), json!(s.error));
    map.insert("pid".into(), json!(std::process::id()));
    map.insert("quotaDisplay".into(), json!(settings.quota_display));
    map.insert("schemaVersion".into(), json!(1));
    map.insert("spending".into(), Value::Array(spending));
    map.insert("stale".into(), json!(s.stale));
    map.insert("summary".into(), json!(s.summary));
    map.insert("updated".into(), json!(s.updated));
    Value::Object(map)
}

// ---------------------------------------------------------------------------
// Request dispatch (DesktopController::listen handler)
// ---------------------------------------------------------------------------

fn configure(settings_json: &Value, shared: &Arc<Mutex<Shared>>) -> (Value, Option<IpcCommand>) {
    let err = |msg: String| (json!({ "ok": false, "error": msg }), None);
    let Some(changes) = settings_json.as_object() else {
        return (json!({ "ok": false, "error": "Unknown command" }), None);
    };
    let mut guard = shared.lock().unwrap();
    let Ok(current) = serde_json::to_value(&guard.settings) else {
        return err("Cannot save application settings.".into());
    };
    let Some(mut merged) = current.as_object().cloned() else {
        return err("Cannot save application settings.".into());
    };
    for (key, value) in changes {
        if !merged.contains_key(key) {
            return err(format!("Unknown setting: {key}"));
        }
        merged.insert(key.clone(), value.clone());
    }
    let next: Settings = match serde_json::from_value(Value::Object(merged)) {
        Ok(next) => next,
        Err(e) => return err(e.to_string()),
    };
    if let Err(e) = next.validate() {
        return err(e);
    }
    if let Err(e) = crate::settings::save(&next) {
        return err(e);
    }
    guard.settings = next;
    (
        json!({ "ok": true, "error": "" }),
        Some(IpcCommand::ReloadSettings),
    )
}

/// Handle one decoded request object; returns the reply value and an optional
/// command for the app. Pure apart from `configure`/`autostart` side effects.
pub fn handle_request(req: &Value, shared: &Arc<Mutex<Shared>>) -> (Value, Option<IpcCommand>) {
    let command = req.get("command").and_then(|v| v.as_str()).unwrap_or("");
    match command {
        "snapshot" | "background" => {
            let guard = shared.lock().unwrap();
            // Panel adapters opt in to spending without opening the window (Qt parity);
            // the app applies the 5-minute cache / 5-second error-retry throttle.
            let wants_spending = req.get("spending").and_then(|v| v.as_bool()) == Some(true);
            (
                snapshot(&guard),
                wants_spending.then_some(IpcCommand::RefreshCostsIfStale),
            )
        }
        "refresh" => (json!({ "ok": true }), Some(IpcCommand::Refresh)),
        "settings" | "usage" | "spending" | "quick-view" | "sessions" => (
            json!({ "ok": true }),
            Some(IpcCommand::Show(command.to_string())),
        ),
        "autostart" => {
            let action = req.get("action").and_then(|v| v.as_str()).unwrap_or("");
            let ok = match action {
                "status" => true,
                "enable" => crate::autostart::set(true).is_ok(),
                "disable" => crate::autostart::set(false).is_ok(),
                _ => false,
            };
            (
                json!({ "ok": ok, "enabled": crate::autostart::status() }),
                None,
            )
        }
        "configure" if req.get("settings").map(|v| v.is_object()).unwrap_or(false) => {
            configure(&req["settings"], shared)
        }
        "quit" => (json!({ "ok": true }), Some(IpcCommand::Quit)),
        _ => (json!({ "ok": false, "error": "Unknown command" }), None),
    }
}

// ---------------------------------------------------------------------------
// Unix-socket server (DesktopController::listen + main.cpp single instance)
// ---------------------------------------------------------------------------

const MAX_REQUEST_BYTES: usize = 65536;

async fn park_forever() {
    std::future::pending::<()>().await;
}

/// Take the single-instance lock; the file handle is leaked so the flock is
/// held for the process lifetime. Returns false when another backend runs.
fn acquire_lock() -> bool {
    let path = crate::ipc::runtime_dir().join("desktop.lock");
    let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&path)
    else {
        return false;
    };
    use std::os::unix::io::AsRawFd;
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc != 0 {
        return false;
    }
    std::mem::forget(file);
    true
}

fn prepare_socket_path() -> Option<std::path::PathBuf> {
    let dir = crate::ipc::runtime_dir();
    std::fs::create_dir_all(&dir).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    if !acquire_lock() {
        return None;
    }
    let path = crate::ipc::socket_path();
    if path.exists() {
        // Remove the socket only when nothing answers on it (stale file).
        match std::os::unix::net::UnixStream::connect(&path) {
            Ok(_) => return None,
            Err(_) => {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    Some(path)
}

async fn serve(
    shared: Arc<Mutex<Shared>>,
    mut tx: iced::futures::channel::mpsc::Sender<IpcCommand>,
) {
    use iced::futures::SinkExt;
    let Some(path) = prepare_socket_path() else {
        park_forever().await;
        return;
    };
    let listener = match tokio::net::UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(_) => {
            park_forever().await;
            return;
        }
    };
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            continue;
        };
        let shared = shared.clone();
        let mut conn_tx = tx.clone();
        tokio::spawn(async move {
            use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
            let (read_half, mut write_half) = stream.into_split();
            let mut reader = BufReader::new(read_half);
            let mut line = String::new();
            let read = tokio::time::timeout(
                std::time::Duration::from_secs(3),
                reader.read_line(&mut line),
            )
            .await;
            let Ok(Ok(n)) = read else { return };
            if n == 0 || line.len() > MAX_REQUEST_BYTES + 1 {
                return;
            }
            let req: Value = serde_json::from_str(line.trim()).unwrap_or_else(|_| json!({}));
            let (reply, cmd) = handle_request(&req, &shared);
            let mut out = serde_json::to_string(&reply).unwrap_or_else(|_| "{}".into());
            out.push('\n');
            if write_half.write_all(out.as_bytes()).await.is_err() {
                return;
            }
            let _ = write_half.flush().await;
            let _ = write_half.shutdown().await;
            // Deliver the command only after the reply is on the wire, so a
            // quit cannot race the client reading its acknowledgement.
            if let Some(cmd) = cmd {
                let _ = conn_tx.send(cmd).await;
            }
        });
        let _ = &mut tx;
    }
}

/// Iced subscription stream: binds the IPC socket on first poll and forwards
/// app commands from socket clients.
pub fn stream(shared: Arc<Mutex<Shared>>) -> iced::futures::stream::BoxStream<'static, IpcCommand> {
    use iced::futures::StreamExt;
    enum State {
        Init(Arc<Mutex<Shared>>),
        Run(iced::futures::channel::mpsc::Receiver<IpcCommand>),
    }
    Box::pin(iced::futures::stream::unfold(
        State::Init(shared),
        |state| async move {
            match state {
                State::Init(shared) => {
                    let (tx, mut rx) = iced::futures::channel::mpsc::channel::<IpcCommand>(16);
                    tokio::spawn(serve(shared, tx));
                    rx.next().await.map(|item| (item, State::Run(rx)))
                }
                State::Run(mut rx) => rx.next().await.map(|item| (item, State::Run(rx))),
            }
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CostEntry, Entry, QuotaWindow};
    use crate::state::Shared;

    fn win(key: &str, minutes: i64, remaining: i64) -> QuotaWindow {
        QuotaWindow {
            key: key.into(),
            label: "Session".into(),
            minutes,
            remaining,
            resets_at: "2999-01-01T00:00:00Z".into(),
            reset_text: String::new(),
            pace: String::new(),
            pace_expected: f64::NAN,
            pace_delta: f64::NAN,
            scoped: false,
        }
    }
    fn sample() -> Shared {
        let mut s = Shared::default();
        s.summary = "CX 99%".into();
        s.updated = "12:34".into();
        s.entries = vec![Entry {
            provider: "codex".into(),
            account_label: "person@example.com".into(),
            plan: "free".into(),
            status: "Operational".into(),
            status_level: "none".into(),
            error: String::new(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            credits: Some(12.5),
            windows: vec![win("primary", 300, 99)],
            details: vec![],
            extra_usage_text: String::new(),
            reset_credits_available: 2,
            reset_credits_next_expiry: "2026-02-01T00:00:00Z".into(),
        }];
        s.spending = vec![CostEntry {
            provider: "codex".into(),
            today: 1.25,
            month: 9.5,
            tokens: 4200,
            provenance: "unknown".into(),
            error: String::new(),
            coverage: String::new(),
            daily: vec![],
        }];
        s
    }

    #[test]
    fn snapshot_golden() {
        let v = snapshot(&sample());
        let obj = v.as_object().unwrap();
        let mut keys: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "barEntries",
                "busy",
                "costBusy",
                "costError",
                "costProviders",
                "entries",
                "error",
                "pid",
                "quotaDisplay",
                "schemaVersion",
                "spending",
                "stale",
                "summary",
                "updated"
            ]
        );
        assert_eq!(v["schemaVersion"], 1);
        assert_eq!(v["pid"], std::process::id());
        assert_eq!(v["summary"], "CX 99%");
        assert_eq!(v["quotaDisplay"], "remaining");
        assert_eq!(v["costProviders"], 1);
        assert_eq!(v["updated"], "12:34");
        let text = serde_json::to_string(&v).unwrap();
        assert!(!text.contains("person@example.com"), "email leaked: {text}");
        assert!(!text.contains("accountLabel") && !text.contains("account_label"));
        let entries = v["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        let e = entries[0].as_object().unwrap();
        let mut ekeys: Vec<&str> = e.keys().map(|k| k.as_str()).collect();
        ekeys.sort();
        assert_eq!(
            ekeys,
            [
                "credits",
                "error",
                "extraUsage",
                "plan",
                "provider",
                "resetCredits",
                "status",
                "statusLevel",
                "updatedAt",
                "windows"
            ]
        );
        assert_eq!(e["plan"], "free");
        assert_eq!(e["credits"], 12.5);
        assert!(e["extraUsage"].is_null());
        assert_eq!(
            e["resetCredits"],
            serde_json::json!({"available":2,"nextExpiresAt":"2026-02-01T00:00:00Z"})
        );
        let w = e["windows"].as_array().unwrap()[0]
            .as_object()
            .unwrap()
            .clone();
        for k in [
            "key",
            "label",
            "minutes",
            "remaining",
            "resetsAt",
            "pace",
            "paceExpected",
            "paceDelta",
            "displayValue",
            "displaySuffix",
            "warning",
            "resetText",
        ] {
            assert!(w.contains_key(k), "missing window key {k}: {w:?}");
        }
        assert_eq!(w["key"], "primary");
        assert_eq!(w["remaining"], 99);
        assert_eq!(w["displayValue"], 99);
        assert_eq!(w["displaySuffix"], "left");
        assert_eq!(w["warning"], false);
        assert!(
            w["paceExpected"].is_null() && w["paceDelta"].is_null(),
            "no pace stays null"
        );
        assert_eq!(
            v["barEntries"],
            serde_json::json!([{"provider":"codex","tag":"CX","text":"99%"}])
        );
        assert_eq!(
            v["spending"],
            serde_json::json!([{"provider":"codex","today":1.25,"month":9.5,"tokens":4200,"provenance":"unknown","error":""}])
        );
    }

    #[test]
    fn snapshot_used_mode_and_warning() {
        let mut s = sample();
        s.settings.quota_display = "used".into();
        s.entries[0].windows[0].remaining = 5;
        let v = snapshot(&s);
        let w = &v["entries"][0]["windows"][0];
        assert_eq!(w["displayValue"], 95);
        assert_eq!(w["displaySuffix"], "used");
        assert_eq!(w["warning"], true);
        assert_eq!(v["barEntries"][0]["text"], "95%");
    }

    #[test]
    fn snapshot_bar_detail_lanes_and_limit() {
        let mut s = sample();
        s.settings.show_bar_detail = true;
        s.settings.bar_providers = 1;
        let mut weekly = win("secondary", 10080, 80);
        weekly.pace = "on pace".into();
        weekly.pace_expected = 49.0;
        weekly.pace_delta = -48.0;
        s.entries[0].windows.push(weekly);
        let mut second = s.entries[0].clone();
        second.provider = "claude".into();
        s.entries.push(second);
        let v = snapshot(&s);
        let bars = v["barEntries"].as_array().unwrap();
        assert_eq!(bars.len(), 1, "barProviders limits bar only");
        assert_eq!(bars[0]["text"], "5H 99% · 7D 80% · -48%");
        assert_eq!(
            v["entries"].as_array().unwrap().len(),
            2,
            "entries not limited"
        );
    }

    #[test]
    fn snapshot_unknown_window_renders_em_dash() {
        let mut s = sample();
        s.entries[0].windows.clear();
        let v = snapshot(&s);
        assert_eq!(v["barEntries"][0]["text"], "—");
    }

    #[test]
    fn snapshot_unknown_sentinels_are_null() {
        let mut s = sample();
        s.entries[0].reset_credits_available = -1;
        s.entries[0].credits = None;
        s.spending[0].today = f64::NAN;
        let v = snapshot(&s);
        assert!(v["entries"][0]["resetCredits"].is_null());
        assert!(v["entries"][0]["credits"].is_null());
        assert!(
            v["spending"][0]["today"].is_null(),
            "unknown today is null, never 0"
        );
        assert_eq!(v["spending"][0]["month"], 9.5);
    }

    #[test]
    fn snapshot_extra_key_hashed() {
        let mut s = sample();
        let mut extra = win("extra:secret-model-id", 10080, 42);
        extra.scoped = true;
        s.entries[0].windows = vec![extra];
        let v1 = snapshot(&s);
        let key = v1["entries"][0]["windows"][0]["key"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(!key.contains("secret-model-id"), "key leaked: {key}");
        let hex = key.strip_prefix("extra:").unwrap();
        assert_eq!(hex.len(), 64);
        assert!(hex
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert_eq!(v1["entries"][0]["windows"][0]["scoped"], true);
        let v2 = snapshot(&s);
        assert_eq!(
            v2["entries"][0]["windows"][0]["key"], key,
            "stable within process"
        );
    }

    #[test]
    fn sha256_vector() {
        assert_eq!(
            hex_digest(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex_digest(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn reset_text_modes() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .timestamp_millis();
        assert_eq!(
            reset_text_at("", now, "countdown"),
            "Reset time unavailable"
        );
        assert_eq!(
            reset_text_at("junk", now, "countdown"),
            "Reset time unavailable"
        );
        assert_eq!(
            reset_text_at("2025-12-31T00:00:00Z", now, "countdown"),
            "Reset due · refresh to update"
        );
        assert_eq!(
            reset_text_at("2026-01-01T00:05:00Z", now, "countdown"),
            "Resets in 5m"
        );
        assert_eq!(
            reset_text_at("2026-01-01T02:30:00Z", now, "countdown"),
            "Resets in 2h 30m"
        );
        assert_eq!(
            reset_text_at("2026-01-03T02:00:00Z", now, "countdown"),
            "Resets in 2d 2h"
        );
        assert!(reset_text_at("2026-01-01T00:05:00Z", now, "absolute").starts_with("Resets "));
        assert!(reset_text_at("2026-01-01T00:05:00Z", now, "both").starts_with("Resets in 5m · "));
    }

    #[test]
    fn request_dispatch() {
        let shared = Arc::new(Mutex::new(sample()));
        let (reply, cmd) = handle_request(&serde_json::json!({"command":"snapshot"}), &shared);
        assert_eq!(reply["schemaVersion"], 1);
        assert!(cmd.is_none());
        let (reply, cmd) = handle_request(&serde_json::json!({"command":"refresh"}), &shared);
        assert_eq!(reply, serde_json::json!({"ok":true}));
        assert_eq!(cmd, Some(crate::state::IpcCommand::Refresh));
        let (reply, cmd) = handle_request(&serde_json::json!({"command":"settings"}), &shared);
        assert_eq!(reply["ok"], true);
        assert_eq!(cmd, Some(crate::state::IpcCommand::Show("settings".into())));
        let (reply, cmd) = handle_request(&serde_json::json!({"command":"quit"}), &shared);
        assert_eq!(reply["ok"], true);
        assert_eq!(cmd, Some(crate::state::IpcCommand::Quit));
        let (reply, cmd) = handle_request(&serde_json::json!({"command":"bogus"}), &shared);
        assert_eq!(
            reply,
            serde_json::json!({"ok":false,"error":"Unknown command"})
        );
        assert!(cmd.is_none());
        let (reply, _) = handle_request(&serde_json::json!({"command":"configure"}), &shared);
        assert_eq!(
            reply["error"], "Unknown command",
            "configure without settings object is unknown, like Qt"
        );
    }
}
