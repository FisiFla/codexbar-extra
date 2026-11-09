//! OpenAI Live Usage: Admin API client, minute-bucket aggregation and view models.
//!
//! Port of `OpenAILiveUsageFetcher` / `OpenAILiveUsageBuilder` / `OpenAILiveUsage` /
//! `OpenAILiveUsageErrorText` (macOS). The admin key is NOT stored here: like the mac app it
//! lives in the shared CodexBar config (`providers[id=openai].apiKey`, written by
//! `codexbar config set-api-key openai`) with `OPENAI_ADMIN_KEY` / `OPENAI_API_KEY` env as
//! fallback. The key is only ever sent in an `Authorization` header, never logged or put in argv.
//! Org rate-limit utilization (limits fetcher, manual limits) is not ported.

use crate::charts::{ChartBar, Segment, SEGMENT_PALETTE};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::io::Read;

pub const WINDOWS: [i64; 3] = [15, 60, 360];
const MAX_SAFE: i64 = 9_007_199_254_740_991;
const MAX_PAGES: usize = 20;
const MAX_BODY: u64 = 8 * 1024 * 1024;
const ENDPOINT: &str = "https://api.openai.com/v1/organization/usage/completions";
pub const ALL_MODELS: &str = "All models";

// ---------------------------------------------------------------------------
// Errors (mac `OpenAILiveUsageError` + `OpenAILiveUsageErrorText`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum LiveError {
    MissingCredential,
    AdminKeyRequired,
    RateLimited,
    ApiFailure(u16),
    ParseFailure,
    InvalidWindow,
    Network(String),
}

/// User-facing text from the mac `OpenAILiveUsageErrorText`; the only change is `>` for the
/// settings path arrows, which the default UI font cannot draw.
pub fn error_text(error: &LiveError) -> String {
    match error {
        LiveError::MissingCredential => {
            "Configure an OpenAI organization Admin API key in Settings > Providers > OpenAI."
                .into()
        }
        LiveError::AdminKeyRequired => {
            "An OpenAI organization Admin API key with usage and project read permissions is required."
                .into()
        }
        LiveError::RateLimited => "OpenAI API is rate limited.".into(),
        LiveError::ApiFailure(code) => format!("OpenAI API failed (HTTP {code})."),
        LiveError::ParseFailure => "Could not parse the OpenAI API response.".into(),
        LiveError::InvalidWindow => {
            "Live usage supports 15-minute, 1-hour, and 6-hour windows.".into()
        }
        LiveError::Network(m) => m.clone(),
    }
}

/// `OpenAILiveUsagePolling.interval`.
pub fn next_interval(error: Option<&LiveError>, previous: u64) -> u64 {
    match error {
        Some(LiveError::RateLimited) => 300.min(previous.max(30) * 2),
        None => 30,
        Some(_) => previous.max(30),
    }
}

// ---------------------------------------------------------------------------
// Credential (shared CodexBar config, then environment)
// ---------------------------------------------------------------------------

#[derive(Clone, PartialEq)]
pub struct Credential {
    api_key: String,
    pub project_id: Option<String>,
}

impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credential")
            .field("api_key", &"[REDACTED]")
            .field("project_id", &self.project_id)
            .finish()
    }
}

/// `SettingsValue.cleaned`: trimmed, surrounding quotes removed, empty is none.
fn cleaned(v: Option<&str>) -> Option<String> {
    let t = v?.trim();
    let t = t
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| t.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
        .unwrap_or(t)
        .trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// Pure resolver. `config` is the shared `config.json` text; `env` looks up a variable.
/// The config key wins over `OPENAI_ADMIN_KEY`, then `OPENAI_API_KEY` (mac order).
pub fn resolve_credential(
    config: Option<&str>,
    env: &dyn Fn(&str) -> Option<String>,
) -> Option<Credential> {
    let provider = config
        .and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok())
        .and_then(|v| {
            v.get("providers")?
                .as_array()?
                .iter()
                .find(|p| p.get("id").and_then(|i| i.as_str()) == Some("openai"))
                .cloned()
        });
    let field = |name: &str| {
        cleaned(
            provider
                .as_ref()
                .and_then(|p| p.get(name))
                .and_then(|v| v.as_str()),
        )
    };
    let api_key = field("apiKey")
        .or_else(|| cleaned(env("OPENAI_ADMIN_KEY").as_deref()))
        .or_else(|| cleaned(env("OPENAI_API_KEY").as_deref()))?;
    let project_id = field("workspaceID").or_else(|| cleaned(env("OPENAI_PROJECT_ID").as_deref()));
    Some(Credential {
        api_key,
        project_id,
    })
}

/// Same lookup order as `CodexBarConfigStore.defaultURL`.
pub fn config_path() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    let var = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
    if let Some(p) = var("CODEXBAR_CONFIG") {
        return Some(PathBuf::from(p));
    }
    if let Some(x) = var("XDG_CONFIG_HOME").filter(|x| x.starts_with('/')) {
        return Some(PathBuf::from(x).join("codexbar/config.json"));
    }
    let home = dirs::home_dir()?;
    let xdg = home.join(".config/codexbar/config.json");
    if xdg.exists() {
        return Some(xdg);
    }
    let legacy = home.join(".codexbar/config.json");
    Some(if legacy.exists() { legacy } else { xdg })
}

pub fn load_credential() -> Option<Credential> {
    let text = config_path().and_then(|p| std::fs::read_to_string(p).ok());
    resolve_credential(text.as_deref(), &|k| std::env::var(k).ok())
}

// ---------------------------------------------------------------------------
// Snapshot model (mac `OpenAILiveUsageSnapshot`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ModelUsage {
    pub name: String,
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub output_tokens: i64,
    pub requests: i64,
    pub batch_tokens: i64,
    pub batch_requests: i64,
}

impl ModelUsage {
    pub fn total_tokens(&self) -> i64 {
        self.input_tokens + self.output_tokens
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Minute {
    /// Unix seconds, minute aligned.
    pub start: i64,
    pub models: Vec<ModelUsage>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Point {
    pub start: i64,
    pub total_tokens: i64,
    pub uncached_input_tokens: i64,
    pub cached_input_tokens: i64,
    pub output_tokens: i64,
    pub requests: i64,
    pub is_in_progress: bool,
    pub is_pending: bool,
    pub batch_tokens: i64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Summary {
    pub latest_tpm: i64,
    pub latest_minute_start: Option<i64>,
    pub five_minute_average_tpm: f64,
    pub peak_tpm: i64,
    pub peak_minute_start: Option<i64>,
    pub total_tokens: i64,
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub output_tokens: i64,
    pub requests: i64,
    pub batch_tokens: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub minutes: Vec<Minute>,
    pub window_minutes: i64,
    pub fetched_at: i64,
    pub project_id: Option<String>,
}

impl Snapshot {
    /// Ranked by reported (non-pending) volume so partial buckets cannot evict visible models;
    /// pending-inclusive volume, then the name, break ties.
    pub fn model_names(&self) -> Vec<String> {
        let frontier = self.reporting_frontier();
        let mut reported: BTreeMap<&str, i64> = BTreeMap::new();
        let mut totals: BTreeMap<&str, i64> = BTreeMap::new();
        for minute in &self.minutes {
            let is_reported = frontier.is_some_and(|f| minute.start <= f);
            for m in &minute.models {
                *totals.entry(&m.name).or_default() += m.total_tokens();
                if is_reported {
                    *reported.entry(&m.name).or_default() += m.total_tokens();
                }
            }
        }
        let mut names: Vec<&str> = totals.keys().copied().collect();
        names.sort_by(|a, b| {
            let (ra, rb) = (
                reported.get(a).copied().unwrap_or(0),
                reported.get(b).copied().unwrap_or(0),
            );
            rb.cmp(&ra)
                .then_with(|| totals[b].cmp(&totals[a]))
                .then_with(|| a.cmp(b))
        });
        names.into_iter().map(String::from).collect()
    }

    /// A shared reporting frontier keeps lagging buckets from looking like a drop to zero.
    pub fn reporting_frontier(&self) -> Option<i64> {
        let complete: Vec<&Minute> = self
            .minutes
            .iter()
            .filter(|m| m.start + 60 <= self.fetched_at)
            .collect();
        let last = complete.last()?;
        let cutoff = last.start - 9 * 60;
        Some(
            complete
                .iter()
                .rev()
                .find(|m| m.start >= cutoff && m.models.iter().any(|x| x.total_tokens() > 0))
                .map_or(last.start, |m| m.start),
        )
    }

    pub fn series(&self, filter: Option<&str>) -> Vec<Point> {
        let frontier = self.reporting_frontier();
        self.minutes
            .iter()
            .map(|minute| {
                let models: Vec<&ModelUsage> = minute
                    .models
                    .iter()
                    .filter(|m| filter.is_none_or(|f| m.name == f))
                    .collect();
                let sum = |f: fn(&ModelUsage) -> i64| models.iter().map(|m| f(m)).sum::<i64>();
                let (input, cached, output) = (
                    sum(|m| m.input_tokens),
                    sum(|m| m.cached_input_tokens),
                    sum(|m| m.output_tokens),
                );
                Point {
                    start: minute.start,
                    total_tokens: input + output,
                    uncached_input_tokens: input - cached,
                    cached_input_tokens: cached,
                    output_tokens: output,
                    requests: sum(|m| m.requests),
                    is_in_progress: minute.start + 60 > self.fetched_at,
                    is_pending: frontier.is_none_or(|f| minute.start > f),
                    batch_tokens: sum(|m| m.batch_tokens),
                }
            })
            .collect()
    }

    pub fn summary(&self, filter: Option<&str>) -> Summary {
        let points = self.series(filter);
        let frontier = self.reporting_frontier();
        let latest = points.iter().find(|p| Some(p.start) == frontier);
        let recent: Vec<&Point> = points
            .iter()
            .filter(|p| frontier.is_some_and(|f| p.start <= f && p.start > f - 5 * 60))
            .collect();
        // Peaks stop at the frontier; the earliest peak wins a tie.
        let mut peak: Option<&Point> = None;
        for p in points.iter().filter(|p| !p.is_pending) {
            if peak.is_none_or(|b| p.total_tokens > b.total_tokens) {
                peak = Some(p);
            }
        }
        Summary {
            latest_tpm: latest.map_or(0, |p| p.total_tokens),
            latest_minute_start: frontier,
            five_minute_average_tpm: if recent.is_empty() {
                0.0
            } else {
                recent.iter().map(|p| p.total_tokens).sum::<i64>() as f64 / recent.len() as f64
            },
            peak_tpm: peak.map_or(0, |p| p.total_tokens),
            peak_minute_start: peak.map(|p| p.start),
            total_tokens: points.iter().map(|p| p.total_tokens).sum(),
            input_tokens: points
                .iter()
                .map(|p| p.uncached_input_tokens + p.cached_input_tokens)
                .sum(),
            cached_input_tokens: points.iter().map(|p| p.cached_input_tokens).sum(),
            output_tokens: points.iter().map(|p| p.output_tokens).sum(),
            requests: points.iter().map(|p| p.requests).sum(),
            batch_tokens: points.iter().map(|p| p.batch_tokens).sum(),
        }
    }
}

// ---------------------------------------------------------------------------
// Response parsing + validated minute grid
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct PageDto {
    data: Vec<BucketDto>,
    #[serde(default)]
    has_more: Option<bool>,
    #[serde(default)]
    next_page: Option<String>,
}

#[derive(Deserialize)]
struct BucketDto {
    start_time: i64,
    results: Vec<ResultDto>,
}

#[derive(Deserialize)]
struct ResultDto {
    model: Option<String>,
    batch: Option<bool>,
    input_tokens: Option<i64>,
    input_cached_tokens: Option<i64>,
    output_tokens: Option<i64>,
    num_model_requests: Option<i64>,
}

impl ResultDto {
    fn usage(&self) -> Result<ModelUsage, LiveError> {
        let (input, cached, output, requests) = (
            self.input_tokens.unwrap_or(0),
            self.input_cached_tokens.unwrap_or(0),
            self.output_tokens.unwrap_or(0),
            self.num_model_requests.unwrap_or(0),
        );
        let total = input
            .checked_add(output)
            .filter(|t| *t <= MAX_SAFE)
            .ok_or(LiveError::ParseFailure)?;
        if [input, cached, output, requests].iter().any(|v| *v < 0) || cached > input {
            return Err(LiveError::ParseFailure);
        }
        let name = self.model.as_deref().unwrap_or("").trim();
        let batch = self.batch == Some(true);
        Ok(ModelUsage {
            name: if name.is_empty() { "unknown" } else { name }.into(),
            input_tokens: input,
            cached_input_tokens: cached,
            output_tokens: output,
            requests,
            batch_tokens: if batch { total } else { 0 },
            batch_requests: if batch { requests } else { 0 },
        })
    }
}

#[derive(Debug, PartialEq)]
pub struct ParsedPage {
    pub buckets: Vec<Minute>,
    /// `Some(cursor)` when `has_more` (the cursor may be empty, which the caller rejects).
    pub next_page: Option<String>,
}

/// Parse one `/organization/usage/completions` page.
pub fn parse_page(body: &[u8]) -> Result<ParsedPage, LiveError> {
    let page: PageDto = serde_json::from_slice(body).map_err(|_| LiveError::ParseFailure)?;
    let mut buckets = Vec::new();
    for b in &page.data {
        if b.start_time < 0 {
            return Err(LiveError::ParseFailure);
        }
        let models = b
            .results
            .iter()
            .map(ResultDto::usage)
            .collect::<Result<Vec<_>, _>>()?;
        buckets.push(Minute {
            start: b.start_time,
            models,
        });
    }
    let next_page = page
        .has_more
        .unwrap_or(false)
        .then(|| page.next_page.unwrap_or_default());
    Ok(ParsedPage { buckets, next_page })
}

fn add(a: i64, b: i64) -> Result<i64, LiveError> {
    a.checked_add(b)
        .filter(|s| *s <= MAX_SAFE)
        .ok_or(LiveError::ParseFailure)
}

/// `OpenAILiveUsageBuilder.build`: a dense grid of `window` minutes ending at the current minute.
/// Buckets outside the grid are dropped; models sharing a minute (batch / non-batch) are merged.
pub fn build(
    buckets: &[Minute],
    window: i64,
    fetched_at: i64,
    project_id: Option<String>,
) -> Result<Snapshot, LiveError> {
    if !WINDOWS.contains(&window) || fetched_at < 0 {
        return Err(LiveError::ParseFailure);
    }
    let current = fetched_at.div_euclid(60) * 60;
    let first = current - (window - 1) * 60;
    let mut grid: BTreeMap<i64, BTreeMap<String, ModelUsage>> = BTreeMap::new();
    for bucket in buckets {
        if bucket.start < 0 {
            return Err(LiveError::ParseFailure);
        }
        let minute = bucket.start.div_euclid(60) * 60;
        for m in &bucket.models {
            let fields = [
                m.input_tokens,
                m.cached_input_tokens,
                m.output_tokens,
                m.requests,
                m.batch_tokens,
                m.batch_requests,
            ];
            if fields.iter().any(|v| *v < 0)
                || m.cached_input_tokens > m.input_tokens
                || m.batch_requests > m.requests
            {
                return Err(LiveError::ParseFailure);
            }
            let total = add(m.input_tokens, m.output_tokens)?;
            if m.batch_tokens > total {
                return Err(LiveError::ParseFailure);
            }
            if minute < first || minute > current {
                continue;
            }
            let slot = grid
                .entry(minute)
                .or_default()
                .entry(m.name.clone())
                .or_insert_with(|| ModelUsage {
                    name: m.name.clone(),
                    ..Default::default()
                });
            slot.input_tokens = add(slot.input_tokens, m.input_tokens)?;
            slot.cached_input_tokens = add(slot.cached_input_tokens, m.cached_input_tokens)?;
            slot.output_tokens = add(slot.output_tokens, m.output_tokens)?;
            slot.requests = add(slot.requests, m.requests)?;
            slot.batch_tokens = add(slot.batch_tokens, m.batch_tokens)?;
            slot.batch_requests = add(slot.batch_requests, m.batch_requests)?;
        }
    }
    let minutes = (0..window)
        .map(|i| {
            let start = first + i * 60;
            Minute {
                start,
                models: grid
                    .remove(&start)
                    .map(|m| m.into_values().collect())
                    .unwrap_or_default(),
            }
        })
        .collect();
    Ok(Snapshot {
        minutes,
        window_minutes: window,
        fetched_at,
        project_id,
    })
}

// ---------------------------------------------------------------------------
// Requests + fetch loop
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct PageRequest {
    pub start: i64,
    pub end: i64,
    pub limit: i64,
    pub cursor: Option<String>,
    pub project_id: Option<String>,
}

/// Query parameters exactly as the mac fetcher sends them (`group_by` repeats).
pub fn query_pairs(r: &PageRequest) -> Vec<(&'static str, String)> {
    let mut q = vec![
        ("start_time", r.start.to_string()),
        ("end_time", r.end.to_string()),
        ("bucket_width", "1m".into()),
        ("group_by", "model".into()),
        ("group_by", "batch".into()),
        ("limit", r.limit.to_string()),
    ];
    if let Some(p) = &r.project_id {
        q.push(("project_ids", p.clone()));
    }
    if let Some(c) = &r.cursor {
        q.push(("page", c.clone()));
    }
    q
}

/// Transport result: HTTP status and body.
pub type HttpResult = Result<(u16, Vec<u8>), LiveError>;

pub fn fetch(
    project_id: Option<String>,
    window: i64,
    now: i64,
    mut transport: impl FnMut(&PageRequest) -> HttpResult,
) -> Result<Snapshot, LiveError> {
    if !WINDOWS.contains(&window) {
        return Err(LiveError::InvalidWindow);
    }
    let current = now.div_euclid(60) * 60;
    let first = current - (window - 1) * 60;
    let mut buckets = Vec::new();
    let mut cursor: Option<String> = None;
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..MAX_PAGES {
        let (status, body) = transport(&PageRequest {
            start: first,
            end: now,
            limit: window,
            cursor: cursor.clone(),
            project_id: project_id.clone(),
        })?;
        match status {
            200..=299 => {}
            401 | 403 => return Err(LiveError::AdminKeyRequired),
            429 => return Err(LiveError::RateLimited),
            s => return Err(LiveError::ApiFailure(s)),
        }
        let page = parse_page(&body)?;
        buckets.extend(page.buckets);
        match page.next_page {
            None => return build(&buckets, window, now, project_id),
            Some(next) if !next.is_empty() && seen.insert(next.clone()) => cursor = Some(next),
            Some(_) => return Err(LiveError::ParseFailure),
        }
    }
    Err(LiveError::ParseFailure)
}

/// Blocking real fetch (call from `spawn_blocking`).
pub fn fetch_live(window: i64) -> Result<Snapshot, LiveError> {
    let cred = load_credential().ok_or(LiveError::MissingCredential)?;
    let now = chrono::Utc::now().timestamp();
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(20))
        .build();
    fetch(cred.project_id.clone(), window, now, |req| {
        let mut r = agent
            .get(ENDPOINT)
            .set("Authorization", &format!("Bearer {}", cred.api_key));
        for (k, v) in query_pairs(req) {
            r = r.query(k, &v);
        }
        let resp = match r.call() {
            Ok(resp) => resp,
            Err(ureq::Error::Status(_, resp)) => resp,
            Err(ureq::Error::Transport(t)) => {
                return Err(LiveError::Network(format!(
                    "Could not reach the OpenAI API ({:?}).",
                    t.kind()
                )))
            }
        };
        let status = resp.status();
        let mut body = Vec::new();
        resp.into_reader()
            .take(MAX_BODY)
            .read_to_end(&mut body)
            .map_err(|_| LiveError::ParseFailure)?;
        Ok((status, body))
    })
}

// ---------------------------------------------------------------------------
// View state + chart shaping
// ---------------------------------------------------------------------------

pub struct LiveState {
    pub window_minutes: i64,
    /// `None` = all models.
    pub filter: Option<String>,
    pub snapshot: Option<Snapshot>,
    pub loading: bool,
    pub error: Option<LiveError>,
    pub last_updated: Option<i64>,
    pub interval_secs: u64,
    pub next_refresh: Option<std::time::Instant>,
    /// Bumped on range change / close so stale responses are dropped.
    pub generation: u64,
}

impl Default for LiveState {
    fn default() -> Self {
        Self {
            window_minutes: 60,
            filter: None,
            snapshot: None,
            loading: false,
            error: None,
            last_updated: None,
            interval_secs: 30,
            next_refresh: None,
            generation: 0,
        }
    }
}

pub const LEGEND_UNCACHED: usize = 0;
pub const LEGEND_OUTPUT: usize = 1;
pub const LEGEND_CACHED: usize = 5;
pub const OTHER_INDEX: usize = SEGMENT_PALETTE.len() - 1;

/// `HH:MM` in local time.
pub fn clock(secs: i64) -> String {
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|d| d.with_timezone(&chrono::Local).format("%H:%M").to_string())
        .unwrap_or_default()
}

fn bar(p: &Point, segments: Vec<Segment>) -> ChartBar {
    ChartBar {
        date: clock(p.start),
        value: (!p.is_pending).then_some(p.total_tokens as f64),
        segments: if p.is_pending { vec![] } else { segments },
    }
}

/// Bars for the chart: one per minute. Minutes still reporting are unknown (gray ticks), never
/// zero. All models stack the top 5 plus "Other"; a single model stacks uncached/cached/output.
/// Returns the legend `(label, palette index)` too.
pub fn chart_bars(s: &Snapshot, filter: Option<&str>) -> (Vec<ChartBar>, Vec<(String, usize)>) {
    let points = s.series(filter);
    let seg = |name: &str, idx: usize, v: i64| Segment {
        name: name.into(),
        color_index: idx,
        value: v as f64,
    };
    if filter.is_some() {
        let legend = vec![
            ("Uncached input".to_string(), LEGEND_UNCACHED),
            ("Cached input".to_string(), LEGEND_CACHED),
            ("Output".to_string(), LEGEND_OUTPUT),
        ];
        let bars = points
            .iter()
            .map(|p| {
                let segments = if p.total_tokens > 0 {
                    vec![
                        seg("Uncached input", LEGEND_UNCACHED, p.uncached_input_tokens),
                        seg("Cached input", LEGEND_CACHED, p.cached_input_tokens),
                        seg("Output", LEGEND_OUTPUT, p.output_tokens),
                    ]
                } else {
                    vec![]
                };
                bar(p, segments)
            })
            .collect();
        return (bars, legend);
    }
    let names = s.model_names();
    let top: Vec<&String> = names.iter().take(OTHER_INDEX - 1).collect();
    let mut legend: Vec<(String, usize)> = top
        .iter()
        .enumerate()
        .map(|(i, n)| ((*n).clone(), i))
        .collect();
    if names.len() > top.len() {
        legend.push(("Other".into(), OTHER_INDEX));
    }
    let bars = points
        .iter()
        .zip(&s.minutes)
        .map(|(p, minute)| {
            let mut by: BTreeMap<usize, i64> = BTreeMap::new();
            for m in &minute.models {
                let idx = top
                    .iter()
                    .position(|n| **n == m.name)
                    .unwrap_or(OTHER_INDEX);
                *by.entry(idx).or_default() += m.total_tokens();
            }
            let segments = legend
                .iter()
                .filter_map(|(name, idx)| {
                    by.get(idx).filter(|v| **v > 0).map(|v| seg(name, *idx, *v))
                })
                .collect();
            bar(p, segments)
        })
        .collect();
    (bars, legend)
}

/// Chart axis / tooltip number format.
pub fn tokens_fmt(v: f64) -> String {
    crate::share::compact_count(v.round() as i64)
}

/// `OpenAILiveUsageView.updatedText`.
pub fn updated_text(updated: i64, now: i64) -> String {
    let secs = (now - updated).max(0);
    if secs < 60 {
        "Updated just now".into()
    } else if secs < 3600 {
        format!("Updated {}m ago", secs / 60)
    } else {
        format!("Updated {}h ago", secs / 3600)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fixture shaped after the Admin API docs for `GET /v1/organization/usage/completions`.
    const PAGE_1: &str = include_str!("../tests/fixtures/openai-usage-completions.json");
    /// 1_700_000_100 is a minute boundary; this is 30 s into that minute.
    const NOW: i64 = 1_700_000_130;

    fn m(name: &str, input: i64, cached: i64, output: i64, req: i64) -> ModelUsage {
        ModelUsage {
            name: name.into(),
            input_tokens: input,
            cached_input_tokens: cached,
            output_tokens: output,
            requests: req,
            ..Default::default()
        }
    }

    #[test]
    fn parses_fixture_page() {
        let page = parse_page(PAGE_1.as_bytes()).unwrap();
        assert_eq!(page.buckets.len(), 4);
        assert_eq!(page.next_page, None);
        let first = &page.buckets[0];
        assert_eq!(first.start, 1_699_999_920);
        assert_eq!(first.models[0].name, "gpt-4o");
        assert_eq!(first.models[0].input_tokens, 1000);
        assert_eq!(first.models[0].cached_input_tokens, 200);
        assert_eq!(first.models[0].batch_tokens, 0);
        // Batch result: tokens/requests land in the batch counters; null model -> "unknown".
        let batch = &page.buckets[1].models[1];
        assert_eq!(batch.name, "unknown");
        assert_eq!((batch.batch_tokens, batch.batch_requests), (150, 1));
        // Missing optional counters default to zero.
        assert_eq!(page.buckets[3].models[0].requests, 0);
    }

    #[test]
    fn rejects_malformed_pages() {
        let bad = [
            "not json",
            r#"{"data":[{"start_time":-1,"results":[]}]}"#,
            r#"{"data":[{"start_time":0,"results":[{"input_tokens":-1}]}]}"#,
            r#"{"data":[{"start_time":0,"results":[{"input_tokens":1,"input_cached_tokens":2}]}]}"#,
            r#"{"data":[{"start_time":0,"results":[{"input_tokens":9007199254740991,"output_tokens":1}]}]}"#,
            r#"{"data":[{"start_time":0,"results":[{"input_tokens":1.5}]}]}"#,
            r#"{"nodata":1}"#,
        ];
        for b in bad {
            assert_eq!(
                parse_page(b.as_bytes()),
                Err(LiveError::ParseFailure),
                "{b}"
            );
        }
    }

    #[test]
    fn has_more_reports_cursor() {
        let p = parse_page(br#"{"data":[],"has_more":true,"next_page":"abc"}"#).unwrap();
        assert_eq!(p.next_page.as_deref(), Some("abc"));
    }

    #[test]
    fn build_grid_merges_models_and_drops_outside() {
        let current = NOW.div_euclid(60) * 60;
        let mut batch = m("a", 50, 0, 10, 1);
        batch.batch_tokens = 60;
        batch.batch_requests = 1;
        let buckets = vec![
            Minute {
                start: current - 60,
                models: vec![m("a", 100, 10, 20, 2), batch],
            },
            Minute {
                start: current - 15 * 60, // outside the 15-minute window
                models: vec![m("old", 1, 0, 1, 1)],
            },
            Minute {
                start: current + 60, // in the future
                models: vec![m("future", 1, 0, 1, 1)],
            },
        ];
        let s = build(&buckets, 15, NOW, Some("proj_1".into())).unwrap();
        assert_eq!(s.minutes.len(), 15);
        assert_eq!(s.minutes[0].start, current - 14 * 60);
        assert_eq!(s.minutes[14].start, current);
        assert!(s.minutes.windows(2).all(|w| w[1].start - w[0].start == 60));
        let row = &s.minutes[13].models;
        assert_eq!(row.len(), 1, "same model in one minute merges");
        assert_eq!(row[0].input_tokens, 150);
        assert_eq!(row[0].cached_input_tokens, 10);
        assert_eq!(row[0].output_tokens, 30);
        assert_eq!(row[0].requests, 3);
        assert_eq!((row[0].batch_tokens, row[0].batch_requests), (60, 1));
        assert_eq!(s.model_names(), ["a"]);
        assert_eq!(s.project_id.as_deref(), Some("proj_1"));
        assert_eq!(
            build(&buckets, 7, NOW, None).unwrap_err(),
            LiveError::ParseFailure
        );
    }

    #[test]
    fn build_rejects_inconsistent_buckets() {
        let current = NOW.div_euclid(60) * 60;
        let mut bad = m("a", 10, 0, 5, 1);
        bad.batch_tokens = 99;
        let r = build(
            &[Minute {
                start: current,
                models: vec![bad],
            }],
            15,
            NOW,
            None,
        );
        assert_eq!(r.unwrap_err(), LiveError::ParseFailure);
    }

    /// Fetched 30 s into minute index 14 (in progress); minutes 0..=13 are complete.
    fn snapshot() -> Snapshot {
        let current = NOW.div_euclid(60) * 60;
        let at = |i: i64| current - (14 - i) * 60;
        build(
            &[
                Minute {
                    start: at(8),
                    models: vec![m("big", 900, 100, 100, 3), m("small", 50, 0, 50, 1)],
                },
                Minute {
                    start: at(10),
                    models: vec![m("big", 400, 0, 100, 2)],
                },
                Minute {
                    start: at(11),
                    models: vec![m("big", 200, 0, 100, 1)],
                },
                // minutes 12/13 empty (reporting lag), 14 in progress with partial data
                Minute {
                    start: at(14),
                    models: vec![m("big", 5, 0, 5, 1)],
                },
            ],
            15,
            NOW,
            None,
        )
        .unwrap()
    }

    #[test]
    fn frontier_is_last_nonzero_minute_within_nine_minutes() {
        let s = snapshot();
        let current = NOW.div_euclid(60) * 60;
        assert_eq!(s.reporting_frontier(), Some(current - 3 * 60)); // index 11
                                                                    // No data inside the lookback: falls back to the last complete minute.
        let empty = build(&[], 15, NOW, None).unwrap();
        assert_eq!(empty.reporting_frontier(), Some(current - 60));
    }

    #[test]
    fn series_marks_pending_and_in_progress() {
        let s = snapshot();
        let pts = s.series(None);
        assert_eq!(pts.len(), 15);
        assert!(!pts[11].is_pending && !pts[11].is_in_progress);
        assert!(pts[12].is_pending && pts[13].is_pending);
        assert!(pts[14].is_pending && pts[14].is_in_progress);
        let only_small = s.series(Some("small"));
        assert_eq!(only_small[8].total_tokens, 100);
        assert_eq!(pts[8].uncached_input_tokens, 800 + 50);
    }

    #[test]
    fn summary_aggregates_and_ignores_pending_peaks() {
        let s = snapshot();
        let sum = s.summary(None);
        let current = NOW.div_euclid(60) * 60;
        assert_eq!(sum.latest_tpm, 300);
        assert_eq!(sum.latest_minute_start, Some(current - 3 * 60));
        assert_eq!(sum.peak_tpm, 1100);
        assert_eq!(sum.peak_minute_start, Some(current - 6 * 60));
        // The total includes the in-progress minute; peak and average do not.
        assert_eq!(sum.total_tokens, 1100 + 500 + 300 + 10);
        assert_eq!(sum.requests, 4 + 2 + 1 + 1);
        assert_eq!(sum.cached_input_tokens, 100);
        assert_eq!(sum.input_tokens, 950 + 400 + 200 + 5);
        assert_eq!(sum.output_tokens, 150 + 100 + 100 + 5);
        // Average over minutes 7..=11: 0, 1100, 0, 500, 300.
        assert_eq!(sum.five_minute_average_tpm, (1100.0 + 500.0 + 300.0) / 5.0);
        let small = s.summary(Some("small"));
        assert_eq!((small.peak_tpm, small.total_tokens), (100, 100));
    }

    #[test]
    fn model_ranking_and_chart_stacks() {
        let s = snapshot();
        assert_eq!(s.model_names(), ["big", "small"]);
        let (bars, legend) = chart_bars(&s, None);
        assert_eq!(bars.len(), 15);
        assert_eq!(legend.len(), 2);
        assert_eq!(bars[8].value, Some(1100.0));
        let names: Vec<_> = bars[8].segments.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["big", "small"]);
        assert_eq!(bars[12].value, None, "reporting minute is unknown, not 0");
        assert_eq!(bars[14].value, None);
        assert_eq!(bars[9].value, Some(0.0));
        let (mb, ml) = chart_bars(&s, Some("big"));
        assert_eq!(ml.len(), 3);
        assert_eq!(mb[8].segments.len(), 3);
        assert_eq!(mb[8].segments[1].value, 100.0);
    }

    #[test]
    fn top_five_fold_into_other() {
        let current = NOW.div_euclid(60) * 60;
        let models: Vec<ModelUsage> = (0..8)
            .map(|i| m(&format!("m{i}"), 100 - i, 0, 0, 1))
            .collect();
        let s = build(
            &[Minute {
                start: current - 120,
                models,
            }],
            15,
            NOW,
            None,
        )
        .unwrap();
        let (bars, legend) = chart_bars(&s, None);
        assert_eq!(legend.len(), SEGMENT_PALETTE.len() - 1);
        assert_eq!(legend.last().unwrap(), &("Other".to_string(), OTHER_INDEX));
        let last = bars[12].segments.last().unwrap();
        assert_eq!(last.name, "Other");
        assert_eq!(last.value, (95 + 94 + 93) as f64);
    }

    #[test]
    fn query_pairs_match_mac_request() {
        let q = query_pairs(&PageRequest {
            start: 10,
            end: 20,
            limit: 60,
            cursor: Some("c1".into()),
            project_id: Some("proj_9".into()),
        });
        let flat: Vec<String> = q.iter().map(|(k, v)| format!("{k}={v}")).collect();
        assert_eq!(
            flat,
            [
                "start_time=10",
                "end_time=20",
                "bucket_width=1m",
                "group_by=model",
                "group_by=batch",
                "limit=60",
                "project_ids=proj_9",
                "page=c1"
            ]
        );
    }

    #[test]
    fn fetch_paginates_and_maps_statuses() {
        let mut calls = Vec::new();
        let snap = fetch(None, 15, NOW, |req| {
            calls.push(req.cursor.clone());
            let current = NOW.div_euclid(60) * 60;
            let body = if req.cursor.is_none() {
                format!(
                    r#"{{"data":[{{"start_time":{},"results":[{{"model":"a","input_tokens":10,"output_tokens":5,"num_model_requests":1}}]}}],"has_more":true,"next_page":"p2"}}"#,
                    current - 60
                )
            } else {
                format!(
                    r#"{{"data":[{{"start_time":{},"results":[{{"model":"a","input_tokens":1,"output_tokens":1,"num_model_requests":1}}]}}],"has_more":false}}"#,
                    current - 120
                )
            };
            Ok((200, body.into_bytes()))
        })
        .unwrap();
        assert_eq!(calls, [None, Some("p2".to_string())]);
        assert_eq!(snap.summary(None).total_tokens, 17);

        let status = |code: u16| fetch(None, 15, NOW, |_| Ok((code, b"{}".to_vec())));
        assert_eq!(status(401).unwrap_err(), LiveError::AdminKeyRequired);
        assert_eq!(status(403).unwrap_err(), LiveError::AdminKeyRequired);
        assert_eq!(status(429).unwrap_err(), LiveError::RateLimited);
        assert_eq!(status(500).unwrap_err(), LiveError::ApiFailure(500));
        assert_eq!(
            fetch(None, 5, NOW, |_| unreachable!()).unwrap_err(),
            LiveError::InvalidWindow
        );
        // A repeated or missing cursor must not loop.
        let looping = fetch(None, 15, NOW, |_| {
            Ok((
                200,
                br#"{"data":[],"has_more":true,"next_page":"same"}"#.to_vec(),
            ))
        });
        assert_eq!(looping.unwrap_err(), LiveError::ParseFailure);
        let no_cursor = fetch(None, 15, NOW, |_| {
            Ok((200, br#"{"data":[],"has_more":true}"#.to_vec()))
        });
        assert_eq!(no_cursor.unwrap_err(), LiveError::ParseFailure);
    }

    #[test]
    fn error_texts_match_mac() {
        assert_eq!(
            error_text(&LiveError::MissingCredential),
            "Configure an OpenAI organization Admin API key in Settings > Providers > OpenAI."
        );
        assert_eq!(
            error_text(&LiveError::AdminKeyRequired),
            "An OpenAI organization Admin API key with usage and project read permissions is required."
        );
        assert_eq!(
            error_text(&LiveError::RateLimited),
            "OpenAI API is rate limited."
        );
        assert_eq!(
            error_text(&LiveError::ApiFailure(503)),
            "OpenAI API failed (HTTP 503)."
        );
        assert_eq!(
            error_text(&LiveError::ParseFailure),
            "Could not parse the OpenAI API response."
        );
        assert_eq!(
            error_text(&LiveError::InvalidWindow),
            "Live usage supports 15-minute, 1-hour, and 6-hour windows."
        );
    }

    #[test]
    fn polling_backoff_matches_mac() {
        assert_eq!(next_interval(None, 120), 30);
        assert_eq!(next_interval(Some(&LiveError::RateLimited), 30), 60);
        assert_eq!(next_interval(Some(&LiveError::RateLimited), 200), 300);
        assert_eq!(next_interval(Some(&LiveError::ApiFailure(500)), 90), 90);
        assert_eq!(next_interval(Some(&LiveError::ApiFailure(500)), 10), 30);
    }

    #[test]
    fn credential_resolution_order_and_redaction() {
        let no_env = |_: &str| None;
        let cfg = r#"{"providers":[{"id":"codex"},{"id":"openai","apiKey":" \"sk-admin-cfg\" ","workspaceID":"proj_a"}]}"#;
        let c = resolve_credential(Some(cfg), &no_env).unwrap();
        assert_eq!(c.api_key, "sk-admin-cfg");
        assert_eq!(c.project_id.as_deref(), Some("proj_a"));
        assert!(!format!("{c:?}").contains("sk-admin-cfg"));

        let env = |k: &str| match k {
            "OPENAI_ADMIN_KEY" => Some("sk-admin-env".to_string()),
            "OPENAI_API_KEY" => Some("sk-plain".to_string()),
            "OPENAI_PROJECT_ID" => Some("proj_env".to_string()),
            _ => None,
        };
        let c = resolve_credential(Some(r#"{"providers":[{"id":"openai"}]}"#), &env).unwrap();
        assert_eq!(c.api_key, "sk-admin-env");
        assert_eq!(c.project_id.as_deref(), Some("proj_env"));
        let only_plain = |k: &str| (k == "OPENAI_API_KEY").then(|| "sk-plain".to_string());
        assert_eq!(
            resolve_credential(None, &only_plain).unwrap().api_key,
            "sk-plain"
        );
        let blank = r#"{"providers":[{"id":"openai","apiKey":"  "}]}"#;
        assert!(resolve_credential(Some(blank), &no_env).is_none());
        assert!(resolve_credential(Some("garbage"), &no_env).is_none());
    }

    #[test]
    fn updated_text_buckets() {
        assert_eq!(updated_text(100, 130), "Updated just now");
        assert_eq!(updated_text(100, 100 + 180), "Updated 3m ago");
        assert_eq!(updated_text(100, 100 + 7300), "Updated 2h ago");
    }
}
