//! Share Stats: payload building, text formatting and PNG save/clipboard helpers.
//! Mirrors Sources/CodexBar/ShareStatsPayload.swift. Only aggregate usage, plan
//! tier and estimated spend are included: never emails or account identity.
//! Unknown stays unknown: unmeasured values render as "—", never 0.

use crate::model::{CostEntry, Entry};
use crate::spend::SpendModel;
use std::collections::BTreeMap;

pub const EM_DASH: &str = "—";

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderRow {
    pub id: String,
    pub name: String,
    pub plan: Option<String>,
    pub tokens: Option<i64>,
    pub cost: Option<f64>,
    pub covered_days: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelRow {
    pub provider_id: String,
    pub provider_name: String,
    pub model: String,
    pub tokens: Option<i64>,
    pub cost: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CurrencyRow {
    pub code: String,
    pub cost: Option<f64>,
    pub covered_days: usize,
    pub partial: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SharePayload {
    /// Requested window in days; 0 means all time.
    pub days: i64,
    /// Last included civil day, `YYYY-MM-DD`.
    pub period_end: String,
    pub providers: Vec<ProviderRow>,
    pub top_models: Vec<ModelRow>,
    pub currencies: Vec<CurrencyRow>,
    pub total_tokens: Option<i64>,
    pub partial_tokens: bool,
    pub partial_models: bool,
}

impl SharePayload {
    pub fn is_all_time(&self) -> bool {
        self.days <= 0
    }
}

// ---------------------------------------------------------------------------
// Sanitizing (closed vocabularies; fail closed)
// ---------------------------------------------------------------------------

/// Plan tier as a closed share-card value. Anything that is not made only of
/// known plan words (so no emails, orgs or free-form names) is dropped.
pub fn subscription_name(plan: &str, account_label: &str) -> Option<String> {
    const WORDS: &[&str] = &[
        "free",
        "plus",
        "pro",
        "max",
        "team",
        "teams",
        "business",
        "enterprise",
        "edu",
        "education",
        "go",
        "lite",
        "ultra",
        "standard",
        "premium",
        "individual",
        "student",
        "starter",
        "basic",
        "paid",
        "prolite",
    ];
    let plan = plan.trim();
    if plan.is_empty() || plan.eq_ignore_ascii_case(account_label.trim()) || plan.contains('@') {
        return None;
    }
    let words: Vec<String> = plan
        .split_whitespace()
        .map(|w| w.to_ascii_lowercase())
        .collect();
    let is_multiplier = |w: &str| {
        w.strip_suffix('x')
            .is_some_and(|n| !n.is_empty() && n.len() <= 3 && n.bytes().all(|b| b.is_ascii_digit()))
    };
    if words.is_empty()
        || words.len() > 3
        || !words
            .iter()
            .all(|w| WORDS.contains(&w.as_str()) || is_multiplier(w))
    {
        return None;
    }
    Some(
        words
            .iter()
            .map(|w| {
                let mut c = w.chars();
                match c.next() {
                    Some(f) if !is_multiplier(w) => {
                        f.to_uppercase().collect::<String>() + c.as_str()
                    }
                    _ => w.clone(),
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// Public model family label, or None when the name is not a recognised
/// public model (paths, ids and private names never leave the machine).
pub fn model_family(raw: &str) -> Option<&'static str> {
    const FAMILIES: &[(&[&str], &str)] = &[
        (&["amazon.nova-", "nova-"], "Amazon Nova"),
        (&["anthropic.claude-", "claude-", "claude "], "Claude"),
        (&["chatgpt-", "gpt-"], "GPT"),
        (&["codex-"], "Codex"),
        (&["command-"], "Command"),
        (&["dall-e-"], "DALL-E"),
        (&["deepseek-"], "DeepSeek"),
        (
            &[
                "codestral-",
                "devstral-",
                "magistral-",
                "mistral-",
                "mistral ",
                "mistral.",
                "mixtral-",
            ],
            "Mistral",
        ),
        (&["gemma-"], "Gemma"),
        (&["google.gemini-", "gemini-", "gemini "], "Gemini"),
        (&["glm-"], "GLM"),
        (&["grok-"], "Grok"),
        (&["kimi-", "moonshot-"], "Kimi"),
        (&["meta.llama", "llama-", "llama "], "Llama"),
        (&["minimax-"], "MiniMax"),
        (&["o1"], "o1"),
        (&["o3"], "o3"),
        (&["o4"], "o4"),
        (&["phi-"], "Phi"),
        (&["qwen"], "Qwen"),
        (&["sonar-"], "Sonar"),
        (&["text-embedding-"], "OpenAI Embeddings"),
        (&["tts-"], "OpenAI TTS"),
        (&["whisper-"], "Whisper"),
    ];
    let value = raw.trim();
    if value.is_empty()
        || value.chars().count() > 72
        || value.contains('@')
        || value.contains("://")
        || value.contains('\\')
        || value.chars().any(|c| c.is_control())
        || value.split_whitespace().count() > 3
        || !value
            .chars()
            .all(|c| c.is_alphanumeric() || " ._+:/()-".contains(c))
        || !(value
            .chars()
            .any(|c| c.is_ascii_digit() || "-_/+.".contains(c)))
    {
        return None;
    }
    let lower = value.to_lowercase();
    let parts: Vec<&str> = lower.split('/').collect();
    let model = match parts.as_slice() {
        [m] => *m,
        [ns, m] if !ns.is_empty() && !m.is_empty() => *m,
        _ => return None,
    };
    let model = ["us.", "eu.", "apac.", "global."]
        .iter()
        .find_map(|p| model.strip_prefix(p))
        .unwrap_or(model);
    FAMILIES
        .iter()
        .find(|(prefixes, _)| prefixes.iter().any(|p| model.starts_with(p)))
        .map(|(_, label)| *label)
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

fn finite_cost(v: f64) -> Option<f64> {
    (v.is_finite() && v >= 0.0).then_some(v)
}

fn window_start(today: &str, days: i64) -> Option<String> {
    if days <= 0 {
        return None;
    }
    let d = chrono::NaiveDate::parse_from_str(today, "%Y-%m-%d").ok()?;
    Some(
        (d - chrono::Duration::days(days - 1))
            .format("%Y-%m-%d")
            .to_string(),
    )
}

/// Builds the card payload, or None when there is nothing shareable
/// (no providers, or no measured tokens or cost at all).
pub fn build(
    spending: &[CostEntry],
    spend: &SpendModel,
    entries: &[Entry],
    days: i64,
    today: &str,
) -> Option<SharePayload> {
    let start = window_start(today, days);
    let in_window = |d: &str| start.as_ref().is_none_or(|s| d >= s.as_str()) && d <= today;

    let mut all_dates: std::collections::BTreeSet<&str> = Default::default();
    let mut providers: Vec<ProviderRow> = spending
        .iter()
        .map(|e| {
            let errored = !e.error.trim().is_empty();
            let rows: Vec<&(String, f64, i64)> =
                e.daily.iter().filter(|(d, _, _)| in_window(d)).collect();
            let (tokens, cost, covered) = if errored {
                (None, None, 0)
            } else if !e.daily.is_empty() {
                if rows.is_empty() {
                    (None, None, 0)
                } else {
                    all_dates.extend(rows.iter().map(|(d, _, _)| d.as_str()));
                    let cost = rows
                        .iter()
                        .try_fold(0.0, |acc, (_, c, _)| finite_cost(*c).map(|c| acc + c));
                    (Some(rows.iter().map(|r| r.2).sum()), cost, rows.len())
                }
            } else {
                // No daily rows: fall back to the loaded 30-day totals, but only
                // when something was measured (zero from no data is unknown).
                (
                    (e.tokens > 0).then_some(e.tokens),
                    finite_cost(e.month).filter(|c| *c > 0.0),
                    0,
                )
            };
            let plan = entries
                .iter()
                .filter(|en| en.provider == e.provider)
                .find_map(|en| subscription_name(&en.plan, &en.account_label));
            ProviderRow {
                id: e.provider.clone(),
                name: crate::model::provider_name(&e.provider),
                plan,
                tokens,
                cost,
                covered_days: covered,
            }
        })
        .collect();
    if !providers
        .iter()
        .any(|p| p.tokens.is_some() || p.cost.is_some())
    {
        return None;
    }
    providers.sort_by(|a, b| {
        let key = |p: &ProviderRow| p.cost.unwrap_or(-1.0);
        key(b)
            .partial_cmp(&key(a))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.name.cmp(&b.name))
    });

    let known_tokens: Vec<i64> = providers.iter().filter_map(|p| p.tokens).collect();
    let total_tokens = (!known_tokens.is_empty()).then(|| known_tokens.iter().sum());
    let partial_tokens = providers.iter().any(|p| p.tokens.is_none());

    let known_costs: Vec<f64> = providers.iter().filter_map(|p| p.cost).collect();
    let currencies = vec![CurrencyRow {
        code: "USD".into(),
        cost: (!known_costs.is_empty()).then(|| known_costs.iter().sum()),
        covered_days: all_dates.len(),
        partial: providers.iter().any(|p| p.cost.is_none()),
    }];

    // Models come from the loaded history (not window-scoped): partial when the
    // window is narrower than what was loaded, or when any family was dropped.
    let mut partial_models = start
        .as_ref()
        .zip(spend.days.first())
        .is_some_and(|(s, (first, _, _))| first < s);
    type Family = (Option<i64>, Vec<Option<f64>>);
    let mut families: BTreeMap<(String, String, String), Family> = BTreeMap::new();
    for (provider, model, cost, tokens, _) in &spend.by_model {
        let Some(family) = model_family(model) else {
            partial_models = true;
            continue;
        };
        let slot = families
            .entry((
                provider.clone(),
                crate::model::provider_name(provider),
                family.to_string(),
            ))
            .or_insert((None, Vec::new()));
        slot.0 = Some(slot.0.unwrap_or(0) + tokens);
        slot.1.push(finite_cost(*cost));
    }
    let mut top_models: Vec<ModelRow> = families
        .into_iter()
        .map(|((pid, pname, model), (tokens, costs))| ModelRow {
            provider_id: pid,
            provider_name: pname,
            model,
            tokens,
            cost: costs
                .iter()
                .all(Option::is_some)
                .then(|| costs.iter().flatten().sum()),
        })
        .collect();
    top_models.sort_by(|a, b| {
        b.tokens
            .cmp(&a.tokens)
            .then_with(|| a.provider_name.cmp(&b.provider_name))
            .then_with(|| a.model.cmp(&b.model))
    });

    Some(SharePayload {
        days,
        period_end: today.to_string(),
        providers,
        top_models,
        currencies,
        total_tokens,
        partial_tokens,
        partial_models,
    })
}

// ---------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------

pub fn compact_count(value: i64) -> String {
    let mag = value.unsigned_abs() as f64;
    let (div, suffix) = if mag >= 1e9 {
        (1e9, "B")
    } else if mag >= 1e6 {
        (1e6, "M")
    } else if mag >= 1e3 {
        (1e3, "K")
    } else {
        return crate::ui::spend_view::count(Some(value));
    };
    let digits = if mag >= div * 100.0 {
        0
    } else if mag >= div * 10.0 {
        1
    } else {
        2
    };
    let mut s = format!("{:.*}", digits, value as f64 / div);
    if s.contains('.') {
        s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    s + suffix
}

pub fn currency(value: f64, code: &str) -> String {
    if code == "USD" {
        crate::ui::spend_view::money(Some(value))
    } else {
        format!("{code} {value:.2}")
    }
}

pub fn subscription_summary(count: usize) -> String {
    if count == 1 {
        "1 subscription".to_string()
    } else {
        format!("{count} subscriptions")
    }
}

/// "Nov 9, 2026" from `YYYY-MM-DD`; the raw string when it does not parse.
pub fn data_through(date: &str) -> String {
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map(|d| d.format("%b %-d, %Y").to_string())
        .unwrap_or_else(|_| date.to_string())
}

pub fn tokens_text(p: &SharePayload) -> String {
    match p.total_tokens {
        None => EM_DASH.to_string(),
        Some(t) if p.partial_tokens => format!("~{}", compact_count(t)),
        Some(t) => compact_count(t),
    }
}

pub fn spend_text(c: &CurrencyRow) -> String {
    match c.cost {
        None => EM_DASH.to_string(),
        Some(v) if c.partial => format!("~{}", currency(v, &c.code)),
        Some(v) => currency(v, &c.code),
    }
}

pub fn provider_detail(p: &ProviderRow, days: i64) -> String {
    let mut metrics: Vec<String> = Vec::new();
    metrics.push(p.tokens.map_or(EM_DASH.to_string(), compact_count));
    match p.cost {
        Some(c) => {
            metrics.push(format!("~{}", currency(c, "USD")));
            if days > 0 && (p.covered_days as i64) < days {
                metrics.push(format!("{}/{}d", p.covered_days, days));
            }
        }
        None => metrics.push("Spend unavailable".to_string()),
    }
    metrics.join(" · ")
}

pub fn model_detail(m: &ModelRow) -> String {
    match (m.cost, m.tokens) {
        (Some(c), _) => format!("~{}", currency(c, "USD")),
        (None, Some(t)) => compact_count(t),
        (None, None) => "used".to_string(),
    }
}

/// Plain-text version (Copy Stats), like ShareStatsFormatting.text.
pub fn text(p: &SharePayload) -> String {
    let fraction = |covered: usize| {
        if p.is_all_time() {
            format!("{covered}/all")
        } else {
            format!("{covered}/{} days", p.days)
        }
    };
    let mut lines = vec![if p.is_all_time() {
        "My AI subscriptions · all time".to_string()
    } else {
        format!("My AI subscriptions · last {} days", p.days)
    }];
    if let Some(t) = p.total_tokens {
        let c = compact_count(t);
        lines.push(if p.partial_tokens {
            format!("~{c} tracked tokens (partial)")
        } else {
            format!("{c} tracked tokens")
        });
    }
    for c in &p.currencies {
        let spend = match c.cost {
            Some(v) => {
                let a = format!("{} estimated", currency(v, &c.code));
                if c.partial {
                    format!("{a} (partial)")
                } else {
                    a
                }
            }
            None => "Spend unavailable".to_string(),
        };
        lines.push(format!(
            "{}: {spend} · coverage {}",
            c.code,
            fraction(c.covered_days)
        ));
    }
    for r in &p.providers {
        let mut m: Vec<String> = Vec::new();
        if let Some(t) = r.tokens {
            m.push(format!("{} tokens", compact_count(t)));
        }
        match r.cost {
            Some(c) => {
                m.push(format!("~{} est", currency(c, "USD")));
                if !p.is_all_time() && (r.covered_days as i64) < p.days {
                    m.push(fraction(r.covered_days));
                }
            }
            None => m.push("Spend unavailable".into()),
        }
        let sub = r
            .plan
            .as_ref()
            .map(|s| format!(" · {s}"))
            .unwrap_or_default();
        lines.push(format!("{}{sub}: {}", r.name, m.join(" · ")));
    }
    if !p.top_models.is_empty() {
        lines.push(
            if p.partial_models {
                "Top models (partial):"
            } else {
                "Top models:"
            }
            .to_string(),
        );
        for m in p.top_models.iter().take(5) {
            let mut x: Vec<String> = Vec::new();
            if let Some(t) = m.tokens {
                x.push(format!("{} tokens", compact_count(t)));
            }
            if let Some(c) = m.cost {
                x.push(format!("~{} est", currency(c, "USD")));
            }
            lines.push(format!(
                "{} ({}): {}",
                m.model,
                m.provider_name,
                x.join(" · ")
            ));
        }
    }
    lines.push(format!(
        "Generated locally by CodexBar · Data through {}",
        data_through(&p.period_end)
    ));
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// PNG save / clipboard
// ---------------------------------------------------------------------------

/// `$XDG_PICTURES_DIR` or `~/Pictures`.
pub fn pictures_dir() -> std::path::PathBuf {
    std::env::var_os("XDG_PICTURES_DIR")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join("Pictures")))
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}

/// `CodexBar-Stats-<date>.png`; a numeric suffix avoids overwriting an earlier save.
pub fn save_path(dir: &std::path::Path, date: &str) -> std::path::PathBuf {
    let first = dir.join(format!("CodexBar-Stats-{date}.png"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| dir.join(format!("CodexBar-Stats-{date}-{n}.png")))
        .find(|p| !p.exists())
        .expect("unbounded range")
}

pub fn save_png(png: &[u8]) -> Result<std::path::PathBuf, String> {
    let dir = pictures_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
    let path = save_path(&dir, &chrono::Local::now().format("%Y-%m-%d").to_string());
    std::fs::write(&path, png).map_err(|e| format!("Cannot write {}: {e}", path.display()))?;
    Ok(path)
}

fn on_path(tool: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(tool).is_file()))
}

/// Clipboard command for the session: wl-copy on Wayland, xclip on X11;
/// the other one is used when the preferred tool is missing.
pub fn clipboard_command(
    wayland: bool,
    x11: bool,
) -> Result<(&'static str, Vec<&'static str>), String> {
    let wl = ("wl-copy", vec!["--type", "image/png"]);
    let xc = (
        "xclip",
        vec!["-selection", "clipboard", "-t", "image/png", "-i"],
    );
    let order = if wayland || !x11 { [wl, xc] } else { [xc, wl] };
    order
        .into_iter()
        .find(|(tool, _)| on_path(tool))
        .ok_or_else(|| {
            "Cannot copy image: neither wl-copy (Wayland, package wl-clipboard) nor xclip (X11) is installed."
                .to_string()
        })
}

pub async fn copy_png(png: Vec<u8>) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty());
    let x11 = std::env::var_os("DISPLAY").is_some_and(|v| !v.is_empty());
    let (tool, args) = clipboard_command(wayland, x11)?;
    let mut child = tokio::process::Command::new(tool)
        .args(&args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Cannot run {tool}: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("no stdin")?;
    stdin
        .write_all(&png)
        .await
        .map_err(|e| format!("{tool}: {e}"))?;
    drop(stdin);
    let status = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait())
        .await
        .map_err(|_| format!("{tool} timed out"))?
        .map_err(|e| format!("{tool}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{tool} failed ({status})"))
    }
}

/// Crops a window screenshot (RGBA, physical pixels) to the card rectangle
/// given in logical pixels and encodes it as PNG.
pub fn crop_to_png(
    rgba: &[u8],
    (width, height): (u32, u32),
    scale: f64,
    [card_x, card_y, card_w, card_h]: [f32; 4],
) -> Result<Vec<u8>, String> {
    let s = scale as f32;
    let (x, y) = ((card_x * s).round() as i32, (card_y * s).round() as i32);
    let (w, h) = ((card_w * s).round() as u32, (card_h * s).round() as u32);
    if x < 0 || y < 0 || x as u32 + w > width || y as u32 + h > height {
        return Err(format!(
            "Window is too small to capture the card ({width}x{height}px); enlarge the window"
        ));
    }
    let size = tiny_skia::IntSize::from_wh(width, height).ok_or("bad screenshot size")?;
    let pixmap = tiny_skia::Pixmap::from_vec(rgba.to_vec(), size).ok_or("bad screenshot data")?;
    let rect = tiny_skia::IntRect::from_xywh(x, y, w, h).ok_or("bad crop rect")?;
    pixmap
        .clone_rect(rect)
        .ok_or("crop failed")?
        .encode_png()
        .map_err(|e| format!("PNG encode failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cost(
        provider: &str,
        month: f64,
        tokens: i64,
        daily: &[(&str, f64, i64)],
        err: &str,
    ) -> CostEntry {
        CostEntry {
            provider: provider.into(),
            today: f64::NAN,
            month,
            tokens,
            provenance: String::new(),
            error: err.into(),
            coverage: String::new(),
            daily: daily
                .iter()
                .map(|(d, c, t)| (d.to_string(), *c, *t))
                .collect(),
        }
    }

    fn entry(provider: &str, plan: &str, label: &str) -> Entry {
        serde_json::from_value(serde_json::json!({
            "provider": provider, "account_label": label, "plan": plan, "status": "",
            "status_level": "", "error": "", "updated_at": "", "credits": null,
            "windows": [], "details": [], "extra_usage_text": "",
            "reset_credits_available": -1, "reset_credits_next_expiry": ""
        }))
        .unwrap()
    }

    const TODAY: &str = "2026-11-09";

    #[test]
    fn empty_data_is_not_shareable() {
        assert_eq!(build(&[], &SpendModel::default(), &[], 30, TODAY), None);
    }

    #[test]
    fn unknown_data_is_not_shareable_and_never_zero() {
        let spending = vec![
            cost("codex", 0.0, 0, &[], ""),
            cost("claude", 0.0, 0, &[], "boom"),
        ];
        assert_eq!(
            build(&spending, &SpendModel::default(), &[], 30, TODAY),
            None
        );
    }

    #[test]
    fn mixed_known_and_unknown() {
        let spending = vec![
            cost(
                "claude",
                0.0,
                0,
                &[("2026-11-08", 2.5, 1_500_000), ("2026-11-09", 1.0, 500_000)],
                "",
            ),
            cost("codex", 0.0, 0, &[], "network"),
        ];
        let p = build(&spending, &SpendModel::default(), &[], 30, TODAY).unwrap();
        assert_eq!(p.total_tokens, Some(2_000_000));
        assert!(p.partial_tokens);
        assert_eq!(tokens_text(&p), "~2M");
        assert_eq!(p.providers[0].id, "claude");
        assert_eq!(p.providers[0].cost, Some(3.5));
        assert_eq!(p.providers[0].covered_days, 2);
        assert_eq!(p.providers[1].tokens, None);
        assert_eq!(p.providers[1].cost, None);
        assert_eq!(
            provider_detail(&p.providers[1], 30),
            "— · Spend unavailable"
        );
        assert_eq!(provider_detail(&p.providers[0], 30), "2M · ~$3.50 · 2/30d");
        assert_eq!(spend_text(&p.currencies[0]), "~$3.50");
        assert_eq!(p.currencies[0].covered_days, 2);
        assert!(p.top_models.is_empty());
    }

    #[test]
    fn unknown_total_shows_dash() {
        // cost measured, tokens unknown -> hero tokens must be an em dash, not 0
        let p = build(
            &[cost("claude", 4.0, 0, &[], "")],
            &SpendModel::default(),
            &[],
            30,
            TODAY,
        )
        .unwrap();
        assert_eq!(p.total_tokens, None);
        assert_eq!(tokens_text(&p), "—");
        assert_eq!(provider_detail(&p.providers[0], 30), "— · ~$4.00 · 0/30d");
        // and the reverse: tokens known, spend unknown
        let p = build(
            &[cost("claude", 0.0, 900, &[], "")],
            &SpendModel::default(),
            &[],
            30,
            TODAY,
        )
        .unwrap();
        assert_eq!(spend_text(&p.currencies[0]), "—");
        assert_eq!(tokens_text(&p), "900");
    }

    #[test]
    fn window_filters_days_and_unmeasured_window_is_unknown() {
        let s = vec![cost(
            "claude",
            0.0,
            0,
            &[("2026-10-01", 9.0, 9000), ("2026-11-09", 1.0, 100)],
            "",
        )];
        let p = build(&s, &SpendModel::default(), &[], 7, TODAY).unwrap();
        assert_eq!(p.providers[0].tokens, Some(100));
        let all = build(&s, &SpendModel::default(), &[], 0, TODAY).unwrap();
        assert_eq!(all.providers[0].tokens, Some(9100));
        assert!(all.is_all_time());
        let none = vec![cost("claude", 0.0, 0, &[("2026-01-01", 1.0, 5)], "")];
        assert_eq!(build(&none, &SpendModel::default(), &[], 7, TODAY), None);
    }

    #[test]
    fn plan_is_closed_vocabulary_and_never_identity() {
        assert_eq!(subscription_name("pro", ""), Some("Pro".into()));
        assert_eq!(subscription_name("Max 20x", ""), Some("Max 20x".into()));
        assert_eq!(subscription_name("me@example.com", ""), None);
        assert_eq!(subscription_name("Acme Corp", ""), None);
        assert_eq!(subscription_name("pro", "PRO"), None);
        let s = vec![cost("claude", 1.0, 10, &[], "")];
        let e = vec![
            entry("claude", "me@example.com", "me@example.com"),
            entry("claude", "Max", "x"),
        ];
        let p = build(&s, &SpendModel::default(), &e, 30, TODAY).unwrap();
        assert_eq!(p.providers[0].plan.as_deref(), Some("Max"));
        let t = text(&p);
        assert!(!t.contains('@'));
    }

    #[test]
    fn models_are_sanitized_to_families() {
        let spend = SpendModel {
            by_model: vec![
                ("claude".into(), "claude-sonnet-4-5".into(), 3.0, 300, 0.5),
                ("claude".into(), "claude-opus-4".into(), 1.0, 100, 0.2),
                (
                    "claude".into(),
                    "/home/me/secret-model-1".into(),
                    1.0,
                    50,
                    0.1,
                ),
                ("codex".into(), "gpt-5".into(), 2.0, 500, 0.3),
            ],
            ..Default::default()
        };
        let p = build(&[cost("claude", 5.0, 450, &[], "")], &spend, &[], 30, TODAY).unwrap();
        assert_eq!(p.top_models.len(), 2);
        assert_eq!(p.top_models[0].model, "GPT");
        assert_eq!(p.top_models[1].model, "Claude");
        assert_eq!(p.top_models[1].tokens, Some(400));
        assert_eq!(p.top_models[1].cost, Some(4.0));
        assert!(p.partial_models);
        assert!(model_family("my-private-model").is_none());
        assert!(model_family("a@b.com-1").is_none());
    }

    #[test]
    fn compact_counts() {
        assert_eq!(compact_count(999), "999");
        assert_eq!(compact_count(1_500), "1.5K");
        assert_eq!(compact_count(12_345), "12.3K");
        assert_eq!(compact_count(123_456), "123K");
        assert_eq!(compact_count(1_000_000), "1M");
        assert_eq!(compact_count(2_340_000_000), "2.34B");
    }

    #[test]
    fn text_and_dates() {
        let p = build(
            &[cost("claude", 0.0, 0, &[("2026-11-09", 1.0, 1000)], "")],
            &SpendModel::default(),
            &[],
            30,
            TODAY,
        )
        .unwrap();
        let t = text(&p);
        assert!(t.starts_with("My AI subscriptions · last 30 days\n1K tracked tokens"));
        assert!(t.ends_with("Data through Nov 9, 2026"));
        assert_eq!(subscription_summary(1), "1 subscription");
        assert_eq!(subscription_summary(3), "3 subscriptions");
    }

    #[test]
    fn save_path_does_not_overwrite() {
        let dir = std::env::temp_dir().join(format!("cbshare-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = save_path(&dir, "2026-11-09");
        assert!(a.ends_with("CodexBar-Stats-2026-11-09.png"));
        std::fs::write(&a, b"x").unwrap();
        assert!(save_path(&dir, "2026-11-09").ends_with("CodexBar-Stats-2026-11-09-2.png"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn crop_rejects_small_windows_and_encodes_png() {
        let rgba = vec![255u8; 40 * 20 * 4];
        let png = crop_to_png(&rgba, (40, 20), 1.0, [4.0, 2.0, 20.0, 10.0]).unwrap();
        assert_eq!(&png[..4], b"\x89PNG");
        assert!(crop_to_png(&rgba, (40, 20), 1.0, [30.0, 2.0, 20.0, 10.0]).is_err());
    }
}
