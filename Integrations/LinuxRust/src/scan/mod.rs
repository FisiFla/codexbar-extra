//! Local session/tool scanners (Codex rollouts, Claude transcripts, OpenCode SQLite, Pi) feeding the
//! Usage & Spend session browser, performance, tool activity, token mix and hourly sections.
//!
//! Privacy: only aggregates and identifiers are ever kept: token counters, cost, model and tool
//! *names*, timestamps, a session id and the last path component of the working directory.
//! Prompt/response text, tool inputs/outputs and titles are parsed past and dropped.
//!
//! Incremental: every source file is reduced to a `FileFacts` record (cheap rows, no text) that is
//! cached in `$XDG_CACHE_HOME/codexbar-linux/session-scan-v1.json` keyed by (mtime, size). Only
//! files whose mtime/size changed are re-parsed; the pricing version is part of the cache key.

pub mod aggregate;
pub mod claude;
pub mod codex;
pub mod opencode;
pub mod pi;
pub mod pricing;

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Instant, UNIX_EPOCH};

pub const CACHE_VERSION: u32 = 1;
pub const CACHE_FILE: &str = "session-scan-v1.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Provider {
    Claude,
    Codex,
    OpenCode,
    Pi,
}

impl Provider {
    pub fn label(self) -> &'static str {
        match self {
            Provider::Claude => "Claude",
            Provider::Codex => "Codex",
            Provider::OpenCode => "OpenCode",
            Provider::Pi => "Pi",
        }
    }
}

/// One billed model request. Token fields are normalised to disjoint counts so that
/// `total = i + cr + cw + o` for every provider (`r` is the reasoning share of `o`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Req {
    /// Stable dedupe key (hash of provider-specific ids); 0 = never deduped.
    #[serde(default)]
    pub k: u64,
    /// Unix milliseconds.
    pub t: i64,
    pub m: String,
    /// Fresh (uncached) input.
    pub i: i64,
    pub o: i64,
    pub cr: i64,
    pub cw: i64,
    #[serde(default)]
    pub r: i64,
    /// USD; `None` = unpriced (unknown, never zero).
    #[serde(default)]
    pub c: Option<f64>,
    /// True when the price came from a same-family fallback rate.
    #[serde(default, skip_serializing_if = "is_false")]
    pub est: bool,
    /// Session id override (defaults to the file's session).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub s: Option<String>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Req {
    pub fn total(&self) -> i64 {
        self.i + self.cr + self.cw + self.o
    }
}

/// One native tool invocation (name and outcome only, never arguments or results).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Tool {
    #[serde(default)]
    pub k: u64,
    pub t: i64,
    pub n: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub e: bool,
    /// Recorded duration in ms.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub d: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub s: Option<String>,
}

/// One timed, successfully completed turn (Codex `task_complete`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Turn {
    /// Completion time, unix ms.
    pub t: i64,
    #[serde(default)]
    pub m: Option<String>,
    #[serde(default)]
    pub e: Option<String>,
    pub d: i64,
    #[serde(default)]
    pub f: Option<i64>,
    pub o: i64,
    #[serde(default)]
    pub i: Option<i64>,
    #[serde(default)]
    pub c: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub s: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileFacts {
    pub provider: Provider,
    /// Default session id for rows without an override.
    pub sid: String,
    /// session id (`""` = default) -> project directory name.
    #[serde(default)]
    pub projects: BTreeMap<String, String>,
    #[serde(default)]
    pub reqs: Vec<Req>,
    #[serde(default)]
    pub tools: Vec<Tool>,
    #[serde(default)]
    pub turns: Vec<Turn>,
}

impl FileFacts {
    pub fn new(provider: Provider, sid: String) -> Self {
        Self {
            provider,
            sid,
            projects: BTreeMap::new(),
            reqs: Vec::new(),
            tools: Vec::new(),
            turns: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CacheEntry {
    pub mtime_ms: i64,
    pub size: u64,
    pub facts: FileFacts,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScanCache {
    pub version: u32,
    pub pricing: u32,
    pub files: BTreeMap<String, CacheEntry>,
}

impl ScanCache {
    fn fresh() -> Self {
        Self {
            version: CACHE_VERSION,
            pricing: pricing::PRICING_VERSION,
            files: BTreeMap::new(),
        }
    }
    /// A cache written by another schema or pricing table is discarded wholesale.
    pub fn load(path: &Path) -> Self {
        let Ok(bytes) = std::fs::read(path) else {
            return Self::fresh();
        };
        match serde_json::from_slice::<ScanCache>(&bytes) {
            Ok(c) if c.version == CACHE_VERSION && c.pricing == pricing::PRICING_VERSION => c,
            _ => Self::fresh(),
        }
    }
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_vec(self).map_err(std::io::Error::other)?,
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        }
        std::fs::rename(&tmp, path)
    }
}

/// `$XDG_CACHE_HOME/codexbar-linux/session-scan-v1.json`.
pub fn default_cache_path() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("codexbar-linux").join(CACHE_FILE))
}

#[derive(Debug, Clone, Default)]
pub struct Roots {
    pub claude: Vec<PathBuf>,
    pub codex: Vec<PathBuf>,
    pub pi: Vec<PathBuf>,
    pub opencode_dbs: Vec<PathBuf>,
}

impl Roots {
    /// Standard locations (honouring `CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `XDG_DATA_HOME`).
    pub fn detect() -> Self {
        let home = dirs::home_dir().unwrap_or_default();
        let mut claude = Vec::new();
        if let Some(v) = std::env::var_os("CLAUDE_CONFIG_DIR") {
            for p in std::env::split_paths(&v.to_string_lossy().replace(',', ":")) {
                claude.push(p.join("projects"));
            }
        }
        claude.push(home.join(".config/claude/projects"));
        claude.push(home.join(".claude/projects"));
        let codex_home = std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".codex"));
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"));
        Self {
            claude: dedupe_paths(claude),
            codex: vec![
                codex_home.join("sessions"),
                codex_home.join("archived_sessions"),
            ],
            pi: vec![
                home.join(".pi/agent/sessions"),
                home.join(".omp/agent/sessions"),
            ],
            opencode_dbs: vec![data.join("opencode/opencode.db")],
        }
    }
}

fn dedupe_paths(v: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    v.into_iter().filter(|p| seen.insert(p.clone())).collect()
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// FNV-1a 64; stable across toolchains (keys are persisted in the cache).
pub fn hash64(parts: &[&str]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for p in parts {
        for b in p.bytes().chain(std::iter::once(0x1f)) {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h.max(1)
}

/// RFC 3339 timestamp -> unix ms.
pub fn parse_ts_ms(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.timestamp_millis())
}

/// Numbers are seconds below 1e12 and milliseconds above (Pi convention).
pub fn epoch_to_ms(v: f64) -> Option<i64> {
    if !v.is_finite() || v <= 0.0 {
        return None;
    }
    Some(if v > 1e12 {
        v as i64
    } else {
        (v * 1000.0) as i64
    })
}

/// Last path component of a working directory; the only path fragment ever stored.
pub fn project_name(cwd: &str) -> Option<String> {
    let trimmed = cwd.trim_end_matches('/');
    let name = trimmed.rsplit('/').next().unwrap_or("");
    if name.is_empty() {
        return None;
    }
    if let Some(home) = dirs::home_dir() {
        if home.to_string_lossy().trim_end_matches('/') == trimmed {
            return Some("~".into());
        }
    }
    Some(name.to_string())
}

pub fn nonneg(v: Option<&serde_json::Value>) -> i64 {
    v.and_then(|x| x.as_i64().or_else(|| x.as_f64().map(|f| f as i64)))
        .unwrap_or(0)
        .max(0)
}

/// Calls `f` for each newline-terminated record without UTF-8 validation or line-size limits.
pub fn for_each_line(path: &Path, mut f: impl FnMut(&[u8])) -> std::io::Result<()> {
    let file = std::fs::File::open(path)?;
    let mut rd = BufReader::with_capacity(256 * 1024, file);
    let mut buf = Vec::with_capacity(16 * 1024);
    loop {
        buf.clear();
        if rd.read_until(b'\n', &mut buf)? == 0 {
            return Ok(());
        }
        while matches!(buf.last(), Some(b'\n' | b'\r')) {
            buf.pop();
        }
        if !buf.is_empty() {
            f(&buf);
        }
    }
}

/// Cheap `"key":"value"` string extraction without a JSON parse (first occurrence).
pub fn quick_str<'a>(line: &'a [u8], key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\":\"");
    let pos = find(line, needle.as_bytes())? + needle.len();
    let rest = &line[pos..];
    let end = rest.iter().position(|b| *b == b'"')?;
    std::str::from_utf8(&rest[..end]).ok()
}

pub fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    let first = needle[0];
    let last = hay.len() - needle.len();
    let mut i = 0;
    while i <= last {
        i += hay[i..=last].iter().position(|b| *b == first)?;
        if &hay[i..i + needle.len()] == needle {
            return Some(i);
        }
        i += 1;
    }
    None
}

pub fn contains(hay: &[u8], needle: &str) -> bool {
    find(hay, needle.as_bytes()).is_some()
}

// ---------------------------------------------------------------------------
// Source enumeration + orchestration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Source {
    provider: Provider,
    path: PathBuf,
    mtime_ms: i64,
    size: u64,
    sqlite: bool,
}

fn file_stamp(path: &Path) -> Option<(i64, u64)> {
    let md = std::fs::metadata(path).ok()?;
    let mtime = md
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis() as i64;
    Some((mtime, md.len()))
}

fn walk_jsonl(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        let p = e.path();
        if ft.is_dir() && depth < 8 {
            walk_jsonl(&p, out, depth + 1);
        } else if ft.is_file() && p.extension().is_some_and(|x| x == "jsonl") {
            out.push(p);
        }
    }
}

fn enumerate(roots: &Roots) -> Vec<Source> {
    let mut out = Vec::new();
    let mut add_jsonl = |provider: Provider, dirs: &[PathBuf]| {
        let mut files = Vec::new();
        for d in dirs {
            walk_jsonl(d, &mut files, 0);
        }
        files.sort();
        files.dedup();
        for path in files {
            if let Some((mtime_ms, size)) = file_stamp(&path) {
                out.push(Source {
                    provider,
                    path,
                    mtime_ms,
                    size,
                    sqlite: false,
                });
            }
        }
    };
    add_jsonl(Provider::Claude, &roots.claude);
    add_jsonl(Provider::Codex, &roots.codex);
    add_jsonl(Provider::Pi, &roots.pi);
    for db in &roots.opencode_dbs {
        if let Some((mut mtime_ms, mut size)) = file_stamp(db) {
            // WAL contents are part of the logical database.
            let wal = PathBuf::from(format!("{}-wal", db.display()));
            if let Some((m, s)) = file_stamp(&wal) {
                mtime_ms = mtime_ms.max(m);
                size += s;
            }
            out.push(Source {
                provider: Provider::OpenCode,
                path: db.clone(),
                mtime_ms,
                size,
                sqlite: true,
            });
        }
    }
    out
}

fn parse_source(s: &Source) -> Option<FileFacts> {
    match s.provider {
        Provider::Claude => claude::parse_file(&s.path).ok(),
        Provider::Codex => codex::parse_file(&s.path).ok(),
        Provider::Pi => pi::parse_file(&s.path).ok(),
        Provider::OpenCode if s.sqlite => opencode::parse_db(&s.path).ok(),
        Provider::OpenCode => None,
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScanStats {
    pub files: usize,
    pub parsed: usize,
    pub cached: usize,
    pub elapsed_ms: u128,
    pub bytes_parsed: u64,
}

/// Scan the standard locations with the standard cache, off the calling thread.
pub async fn scan_default() -> ScanSnapshot {
    tokio::task::spawn_blocking(|| scan(&Roots::detect(), default_cache_path().as_deref()))
        .await
        .unwrap_or_default()
}

/// Start of the period in unix ms: local midnight of the first of the last `days` calendar days
/// (same window as the daily trend); `None` for "All" (`days <= 0`).
pub fn period_since_ms(days: i64) -> Option<i64> {
    if days <= 0 {
        return None;
    }
    use chrono::{Duration, Local, TimeZone};
    let first = Local::now().date_naive() - Duration::days(days - 1);
    Local
        .from_local_datetime(&first.and_hms_opt(0, 0, 0)?)
        .earliest()
        .map(|d| d.timestamp_millis())
}

/// Facts merged across files with duplicates removed (resumed/forked transcripts repeat rows).
#[derive(Debug, Clone, Default)]
pub struct ScanSnapshot {
    pub reqs: Vec<(Provider, String, Req)>,
    pub tools: Vec<(Provider, String, Tool)>,
    pub turns: Vec<(Provider, String, Turn)>,
    pub projects: BTreeMap<(Provider, String), String>,
    pub stats: ScanStats,
}

/// Scan every source under `roots`, reusing `cache_path` for unchanged files.
pub fn scan(roots: &Roots, cache_path: Option<&Path>) -> ScanSnapshot {
    let started = Instant::now();
    let mut cache = cache_path
        .map(ScanCache::load)
        .unwrap_or_else(ScanCache::fresh);
    let sources = enumerate(roots);
    let mut stats = ScanStats {
        files: sources.len(),
        ..Default::default()
    };

    let mut todo: Vec<&Source> = Vec::new();
    for s in &sources {
        let key = s.path.to_string_lossy().to_string();
        match cache.files.get(&key) {
            Some(e)
                if e.mtime_ms == s.mtime_ms
                    && e.size == s.size
                    && e.facts.provider == s.provider =>
            {
                stats.cached += 1
            }
            _ => todo.push(s),
        }
    }
    // Largest first so the worker pool finishes evenly.
    todo.sort_by_key(|s| std::cmp::Reverse(s.size));
    stats.parsed = todo.len();
    stats.bytes_parsed = todo.iter().map(|s| s.size).sum();

    let parsed: Mutex<Vec<(String, CacheEntry)>> = Mutex::new(Vec::new());
    let next = AtomicUsize::new(0);
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        .clamp(1, 8)
        .min(todo.len().max(1));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(src) = todo.get(i) else { break };
                if let Some(facts) = parse_source(src) {
                    let entry = CacheEntry {
                        mtime_ms: src.mtime_ms,
                        size: src.size,
                        facts,
                    };
                    parsed
                        .lock()
                        .unwrap()
                        .push((src.path.to_string_lossy().to_string(), entry));
                }
            });
        }
    });
    let changed = !todo.is_empty();
    for (k, v) in parsed.into_inner().unwrap() {
        cache.files.insert(k, v);
    }
    // Forget vanished files.
    let live: HashSet<String> = sources
        .iter()
        .map(|s| s.path.to_string_lossy().to_string())
        .collect();
    let before = cache.files.len();
    cache.files.retain(|k, _| live.contains(k));
    if let Some(p) = cache_path {
        if changed || cache.files.len() != before {
            let _ = cache.save(p);
        }
    }
    let mut snap = merge(cache.files.values().map(|e| &e.facts));
    stats.elapsed_ms = started.elapsed().as_millis();
    snap.stats = stats;
    snap
}

/// Flatten per-file facts, dropping rows whose dedupe key was already seen.
pub fn merge<'a>(files: impl Iterator<Item = &'a FileFacts>) -> ScanSnapshot {
    let mut snap = ScanSnapshot::default();
    let mut seen_req: HashSet<(Provider, u64)> = HashSet::new();
    let mut seen_tool: HashSet<(Provider, u64)> = HashSet::new();
    for f in files {
        for (sid, project) in &f.projects {
            let sid = if sid.is_empty() { &f.sid } else { sid };
            snap.projects
                .entry((f.provider, sid.clone()))
                .or_insert_with(|| project.clone());
        }
        for r in &f.reqs {
            if r.k != 0 && !seen_req.insert((f.provider, r.k)) {
                continue;
            }
            let sid = r.s.clone().unwrap_or_else(|| f.sid.clone());
            snap.reqs.push((f.provider, sid, r.clone()));
        }
        for t in &f.tools {
            if t.k != 0 && !seen_tool.insert((f.provider, t.k)) {
                continue;
            }
            let sid = t.s.clone().unwrap_or_else(|| f.sid.clone());
            snap.tools.push((f.provider, sid, t.clone()));
        }
        for t in &f.turns {
            let sid = t.s.clone().unwrap_or_else(|| f.sid.clone());
            snap.turns.push((f.provider, sid, t.clone()));
        }
    }
    snap
}

/// Insert `req` into `facts`, keeping the *last* row for a dedupe key (streamed transcripts
/// repeat cumulative usage). `index` maps key -> position.
pub fn upsert_req(
    facts: &mut FileFacts,
    index: &mut std::collections::HashMap<u64, usize>,
    req: Req,
) {
    if req.k != 0 {
        if let Some(&i) = index.get(&req.k) {
            facts.reqs[i] = req;
            return;
        }
        index.insert(req.k, facts.reqs.len());
    }
    facts.reqs.push(req);
}

#[cfg(test)]
pub(crate) mod testutil {
    use std::path::{Path, PathBuf};

    pub fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "codexbar-scan-test-{}-{}-{:?}",
            tag,
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    pub fn write_lines(path: &Path, lines: &[String]) {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).unwrap();
        }
        std::fs::write(path, lines.join("\n") + "\n").unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use testutil::*;

    fn claude_line(msg: &str, req: &str, ts: &str, out: i64) -> String {
        format!(
            r#"{{"type":"assistant","timestamp":"{ts}","sessionId":"sess-a","cwd":"/work/demo","requestId":"{req}","message":{{"id":"{msg}","model":"claude-opus-4-8","role":"assistant","stop_reason":"end_turn","content":[{{"type":"text","text":"x"}}],"usage":{{"input_tokens":10,"cache_creation_input_tokens":0,"cache_read_input_tokens":100,"output_tokens":{out}}}}}}}"#
        )
    }

    /// Manual timing against the real home directory (read-only):
    /// `cargo test --release real_data_timing -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_data_timing() {
        let cache = tmpdir("real").join("cache.json");
        let roots = Roots::detect();
        for label in ["cold", "warm"] {
            let s = scan(&roots, Some(&cache));
            let v = aggregate::aggregate(
                &s,
                aggregate::Filter {
                    since_ms: None,
                    zone: aggregate::Zone::Utc,
                },
            );
            eprintln!(
                "{label}: {:?} reqs={} tools={} turns={} sessions={} agg_ok={}",
                s.stats,
                s.reqs.len(),
                s.tools.len(),
                s.turns.len(),
                v.sessions.len(),
                v.requests == s.reqs.len()
            );
        }
        eprintln!(
            "cache size: {} bytes",
            std::fs::metadata(&cache).map(|m| m.len()).unwrap_or(0)
        );
    }

    #[test]
    fn hash_is_stable_and_nonzero() {
        assert_eq!(hash64(&["a", "b"]), hash64(&["a", "b"]));
        assert_ne!(hash64(&["a", "b"]), hash64(&["ab"]));
        assert_ne!(hash64(&[]), 0);
    }

    #[test]
    fn project_name_keeps_only_last_component() {
        assert_eq!(project_name("/srv/x/demo/"), Some("demo".into()));
        assert_eq!(project_name("/"), None);
    }

    #[test]
    fn quick_str_and_find() {
        let l = br#"{"a":1,"timestamp":"2026-01-01T00:00:00Z","b":2}"#;
        assert_eq!(quick_str(l, "timestamp"), Some("2026-01-01T00:00:00Z"));
        assert_eq!(quick_str(l, "nope"), None);
        assert!(contains(l, "\"b\":2"));
    }

    #[test]
    fn cache_reuses_unchanged_and_reparses_changed_files() {
        let root = tmpdir("cache");
        let proj = root.join("claude/p1");
        let file = proj.join("sess-a.jsonl");
        write_lines(
            &file,
            &[claude_line("m1", "r1", "2026-11-01T10:00:00.000Z", 5)],
        );
        let cache = root.join("cache/scan.json");
        let roots = Roots {
            claude: vec![root.join("claude")],
            ..Default::default()
        };
        let a = scan(&roots, Some(&cache));
        assert_eq!((a.stats.parsed, a.stats.cached), (1, 0));
        assert_eq!(a.reqs.len(), 1);
        // second run: nothing changed -> served from cache, identical facts
        let b = scan(&roots, Some(&cache));
        assert_eq!((b.stats.parsed, b.stats.cached), (1 - 1, 1));
        assert_eq!(b.reqs, a.reqs);
        // grow the file: size changes -> reparsed, new row picked up
        write_lines(
            &file,
            &[
                claude_line("m1", "r1", "2026-11-01T10:00:00.000Z", 5),
                claude_line("m2", "r2", "2026-11-01T10:05:00.000Z", 7),
            ],
        );
        let c = scan(&roots, Some(&cache));
        assert_eq!((c.stats.parsed, c.stats.cached), (1, 0));
        assert_eq!(c.reqs.len(), 2);
        // deleting the source drops it from the cache and the snapshot
        std::fs::remove_file(&file).unwrap();
        let d = scan(&roots, Some(&cache));
        assert!(d.reqs.is_empty());
        let on_disk = ScanCache::load(&cache);
        assert!(on_disk.files.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn cache_is_discarded_on_pricing_or_schema_version_change() {
        let root = tmpdir("ver");
        let path = root.join("c.json");
        let mut c = ScanCache::fresh();
        c.files.insert(
            "x".into(),
            CacheEntry {
                mtime_ms: 1,
                size: 1,
                facts: FileFacts::new(Provider::Claude, "s".into()),
            },
        );
        c.save(&path).unwrap();
        assert_eq!(ScanCache::load(&path).files.len(), 1);
        c.pricing += 1;
        c.save(&path).unwrap();
        assert!(ScanCache::load(&path).files.is_empty());
        c.pricing = pricing::PRICING_VERSION;
        c.version += 1;
        c.save(&path).unwrap();
        assert!(ScanCache::load(&path).files.is_empty());
        std::fs::write(&path, b"not json").unwrap();
        assert!(ScanCache::load(&path).files.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn merge_drops_duplicate_rows_across_files() {
        let mut a = FileFacts::new(Provider::Claude, "s1".into());
        let mut b = FileFacts::new(Provider::Claude, "s2".into());
        let row = Req {
            k: 42,
            t: 1,
            m: "m".into(),
            i: 1,
            ..Default::default()
        };
        a.reqs.push(row.clone());
        b.reqs.push(row);
        b.reqs.push(Req {
            k: 43,
            t: 2,
            m: "m".into(),
            i: 1,
            ..Default::default()
        });
        let snap = merge([&a, &b].into_iter());
        assert_eq!(snap.reqs.len(), 2);
        assert_eq!(snap.reqs[0].1, "s1");
    }

    #[test]
    fn cache_file_holds_no_text() {
        let root = tmpdir("priv");
        let file = root.join("claude/p/sess-a.jsonl");
        let secret = "TOP-SECRET-PROMPT-TEXT";
        let line = format!(
            r#"{{"type":"assistant","timestamp":"2026-11-01T10:00:00.000Z","sessionId":"sess-a","cwd":"/work/demo","requestId":"r","message":{{"id":"m","model":"claude-opus-4-8","role":"assistant","content":[{{"type":"text","text":"{secret}"}},{{"type":"tool_use","id":"t1","name":"Bash","input":{{"command":"{secret}"}}}}],"usage":{{"input_tokens":1,"output_tokens":2}}}}}}"#
        );
        write_lines(&file, &[line]);
        let cache = root.join("c.json");
        let roots = Roots {
            claude: vec![root.join("claude")],
            ..Default::default()
        };
        let snap = scan(&roots, Some(&cache));
        assert_eq!(snap.tools.len(), 1);
        let raw = std::fs::read_to_string(&cache).unwrap();
        assert!(!raw.contains(secret));
        assert!(!raw.contains("/work"));
        assert!(raw.contains("demo"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
