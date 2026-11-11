//! Typed views over Swift `codexbar` CLI JSON (usage / cost / dashboard / config providers).
//! Field names mirror `Sources/CodexBarCLI/CLIPayloads.swift` + `CLICostCommand.swift`.
//! Optional fields are omitted (not null) when absent, hence `Option` + `#[serde(default)]`.

use serde::{Deserialize, Serialize};

/// The CLI emits `error` as `{"message","code","kind"}`; older payloads/fixtures use a bare string.
fn de_error<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    let v = Option::<serde_json::Value>::deserialize(d)?;
    Ok(match v {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(s)) => Some(s),
        Some(serde_json::Value::Object(o)) => Some(
            o.get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error")
                .to_string(),
        ),
        Some(other) => Some(other.to_string()),
    })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsagePayload {
    pub provider: String,
    #[serde(default)]
    pub account: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub status: Option<ProviderStatus>,
    #[serde(default)]
    pub usage: Option<UsageSnapshot>,
    #[serde(default)]
    pub rate_window_labels: Option<RateWindowLabels>,
    #[serde(default)]
    pub credits: Option<Credits>,
    #[serde(default)]
    pub reset_credits: Option<ResetCredits>,
    #[serde(default, deserialize_with = "de_error")]
    pub error: Option<String>,
    #[serde(default)]
    pub pace: Option<std::collections::BTreeMap<String, Pace>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    pub indicator: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateWindowLabels {
    #[serde(default)]
    pub primary: Option<String>,
    #[serde(default)]
    pub secondary: Option<String>,
    #[serde(default)]
    pub tertiary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Credits {
    pub remaining: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetCredits {
    pub available: i64,
    #[serde(default)]
    pub next_expires_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pace {
    pub stage: String,
    pub delta_percent: f64,
    pub expected_used_percent: f64,
    pub will_last_to_reset: bool,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateWindow {
    pub used_percent: f64,
    /// Absent for some providers (e.g. credit-style windows); 0 = unknown cadence.
    #[serde(default)]
    pub window_minutes: i64,
    #[serde(default)]
    pub resets_at: Option<String>,
    #[serde(default)]
    pub reset_description: Option<String>,
    #[serde(default)]
    pub is_synthetic_placeholder: Option<bool>,
}

impl RateWindow {
    /// Remaining percent, clamped like Usage.js `rows()` (100 - used, 0..=100, rounded).
    pub fn remaining_percent(&self) -> i64 {
        (100.0 - self.used_percent).clamp(0.0, 100.0).round() as i64
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    #[serde(default)]
    pub account_email: Option<String>,
    #[serde(default)]
    pub login_method: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetailRow {
    pub label: String,
    pub value: String,
    #[serde(default)]
    pub secondary_value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetailSection {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub rows: Vec<DetailRow>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtraRateWindow {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub usage_known: Option<bool>,
    #[serde(default)]
    pub window: Option<RateWindow>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSnapshot {
    #[serde(default)]
    pub account_email: Option<String>,
    #[serde(default)]
    pub login_method: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub identity: Option<Identity>,
    #[serde(default)]
    pub primary: Option<RateWindow>,
    #[serde(default)]
    pub secondary: Option<RateWindow>,
    #[serde(default)]
    pub tertiary: Option<RateWindow>,
    #[serde(default)]
    pub extra_rate_windows: Vec<ExtraRateWindow>,
    #[serde(default)]
    pub details: Vec<DetailSection>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostPayload {
    pub provider: String,
    #[serde(default, deserialize_with = "de_error")]
    pub error: Option<String>,
    #[serde(default)]
    pub currency_code: Option<String>,
    #[serde(default)]
    pub last30_days_tokens: i64,
    #[serde(default, rename = "last30DaysCostUSD")]
    pub last30_days_cost_usd: f64,
    #[serde(default)]
    pub history_coverage_is_established: bool,
    #[serde(default)]
    pub provenance: Option<String>,
    #[serde(default)]
    pub daily: Vec<CostDay>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostModelBreakdown {
    pub model_name: String,
    #[serde(default)]
    pub cost: f64,
    #[serde(default)]
    pub total_tokens: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostDay {
    pub date: String,
    #[serde(default)]
    pub total_tokens: i64,
    #[serde(default)]
    pub total_cost: f64,
    #[serde(default)]
    pub model_breakdowns: Vec<CostModelBreakdown>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCatalogEntry {
    pub provider: String,
    pub display_name: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub authentication: ProviderAuth,
}

/// Which credentials `config set-api-key` / `set-cookie` accept for a provider.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProviderAuth {
    #[serde(default, rename = "supportsConfigAPIKey")]
    pub api_key: bool,
    #[serde(default, rename = "supportsConfigCookieHeader")]
    pub cookie_header: bool,
}

pub fn parse_usage(bytes: &[u8]) -> Result<Vec<UsagePayload>, serde_json::Error> {
    serde_json::from_slice(bytes)
}
pub fn parse_cost(bytes: &[u8]) -> Result<Vec<CostPayload>, serde_json::Error> {
    serde_json::from_slice(bytes)
}
pub fn parse_providers(bytes: &[u8]) -> Result<Vec<ProviderCatalogEntry>, serde_json::Error> {
    serde_json::from_slice(bytes)
}

/// Build argv for one usage fetch, matching `Integrations/Linux/Shared/Usage.js` command().
pub fn usage_argv(exe: &str, provider: &str, source: &str, show_status: bool) -> Vec<String> {
    let mut v = vec![
        exe.into(),
        "usage".into(),
        "--format".into(),
        "json".into(),
        "--json-only".into(),
    ];
    if provider != "enabled" {
        v.push("--provider".into());
        v.push(provider.into());
    }
    if source != "auto" {
        v.push("--source".into());
        v.push(source.into());
    }
    if show_status {
        v.push("--status".into());
    }
    v
}

/// Qt `DesktopController::probe` limits: 60s deadline, stdout capped at 8MiB.
pub const FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
pub const MAX_STDOUT_BYTES: usize = 8 * 1024 * 1024;

pub fn check_stdout_cap(stdout: &[u8], cap: usize) -> Result<(), String> {
    if stdout.len() > cap {
        Err(format!(
            "CLI output too large ({} bytes, cap {cap} bytes)",
            stdout.len()
        ))
    } else {
        Ok(())
    }
}

/// Spawn one CLI invocation with a hard timeout and stdout size cap.
/// `kill_on_drop` makes the timeout actually terminate the child, like Qt's deadline timer.
/// Secrets go through `stdin`, never argv.
async fn run_command(
    exe: &str,
    args: &[String],
    stdin: Option<&str>,
    timeout: std::time::Duration,
    cap: usize,
    lenient: bool,
) -> Result<Vec<u8>, String> {
    use tokio::io::AsyncWriteExt;
    let fut = async {
        let mut child = tokio::process::Command::new(exe)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| e.to_string())?;
        if let Some(data) = stdin {
            if let Some(mut pipe) = child.stdin.take() {
                pipe.write_all(data.as_bytes())
                    .await
                    .map_err(|e| e.to_string())?;
            }
        }
        child.wait_with_output().await.map_err(|e| e.to_string())
    };
    let out = tokio::time::timeout(timeout, fut)
        .await
        .map_err(|_| format!("CLI call timed out after {}s", timeout.as_secs()))??;
    // Qt parity (DesktopController::probe): read paths only fail on a crash/signal, because the
    // CLI exits non-zero when any single provider errors while still printing every row.
    let normal_exit_with_output =
        out.status.code().is_some() && !out.stdout.trim_ascii().is_empty();
    if !out.status.success() && !(lenient && normal_exit_with_output) {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() {
            format!("CLI exited with {}", out.status)
        } else {
            err
        });
    }
    check_stdout_cap(&out.stdout, cap)?;
    Ok(out.stdout)
}

async fn run_cli(argv: &[String], stdin: Option<&str>) -> Result<Vec<u8>, String> {
    run_command(
        &argv[0],
        &argv[1..],
        stdin,
        FETCH_TIMEOUT,
        MAX_STDOUT_BYTES,
        false,
    )
    .await
}

/// Read-only fetches (usage/cost/dashboard/sessions): accept partial output on non-zero exit.
async fn run_cli_read(argv: &[String]) -> Result<Vec<u8>, String> {
    run_command(
        &argv[0],
        &argv[1..],
        None,
        FETCH_TIMEOUT,
        MAX_STDOUT_BYTES,
        true,
    )
    .await
}

/// Selection ids that fan out themselves; their rows must not be filtered to one provider.
fn is_selection_id(provider: &str) -> bool {
    matches!(provider, "enabled" | "all" | "both" | "custom")
}

async fn fetch_usage_payloads(
    exe: &str,
    provider: &str,
    source: &str,
    show_status: bool,
) -> Result<Vec<UsagePayload>, String> {
    let argv = usage_argv(exe, provider, source, show_status);
    let stdout = run_cli_read(&argv).await?;
    parse_usage(&stdout).map_err(|e| e.to_string())
}

/// Sequential per-provider fan-out (Qt `provider == "custom"` over providerOrder).
/// Rows for a concrete provider are filtered to that provider (a CLI that does not
/// recognise the id may answer for others; never show those under this one).
/// A failed provider contributes one error payload so the UI can render it as
/// unavailable instead of dropping it; only a total failure is `Err`.
async fn fetch_payloads_ordered(
    exe: &str,
    providers: &[String],
    source: &str,
    show_status: bool,
) -> Result<Vec<UsagePayload>, String> {
    if providers.is_empty() {
        return Err("no providers configured".into());
    }
    let mut all = Vec::new();
    let mut first_err: Option<String> = None;
    let mut any_ok = false;
    for provider in providers {
        match fetch_usage_payloads(exe, provider, source, show_status).await {
            Ok(rows) => {
                any_ok = true;
                if is_selection_id(provider) {
                    all.extend(rows);
                } else {
                    all.extend(rows.into_iter().filter(|r| &r.provider == provider));
                }
            }
            Err(e) => {
                if first_err.is_none() {
                    first_err = Some(e);
                }
                if !is_selection_id(provider) {
                    all.push(UsagePayload {
                        provider: provider.clone(),
                        account: None,
                        version: None,
                        source: None,
                        status: None,
                        usage: None,
                        rate_window_labels: None,
                        credits: None,
                        reset_credits: None,
                        error: Some("Usage unavailable".into()),
                        pace: None,
                    });
                }
            }
        }
    }
    if any_ok {
        Ok(all)
    } else {
        Err(first_err.unwrap_or_else(|| "usage fetch failed".into()))
    }
}

pub async fn fetch_entries_ordered(
    exe: &str,
    providers: &[String],
    source: &str,
    show_status: bool,
    show_identity: bool,
) -> Result<Vec<crate::model::Entry>, String> {
    let rows = fetch_payloads_ordered(exe, providers, source, show_status).await?;
    Ok(crate::model::derive_entries(&rows, show_identity))
}
pub async fn fetch_entries(
    exe: &str,
    provider: &str,
    source: &str,
    show_status: bool,
    show_identity: bool,
) -> Result<Vec<crate::model::Entry>, String> {
    fetch_entries_ordered(
        exe,
        &[provider.to_string()],
        source,
        show_status,
        show_identity,
    )
    .await
}
pub async fn fetch_costs(
    exe: &str,
) -> Result<(Vec<crate::model::CostEntry>, Vec<CostPayload>), String> {
    let argv: Vec<String> = [
        exe,
        "cost",
        "--provider",
        "both",
        "--format",
        "json",
        "--days",
        "30",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let stdout = run_cli_read(&argv).await?;
    let payloads = parse_cost(&stdout).map_err(|e| e.to_string())?;
    Ok((crate::model::derive_costs(&payloads), payloads))
}

// --- dashboard / sessions ---------------------------------------------

pub fn dashboard_argv(exe: &str) -> Vec<String> {
    vec![exe.into(), "dashboard".into()]
}
pub fn sessions_argv(exe: &str) -> Vec<String> {
    vec![exe.into(), "sessions".into(), "--json-v2".into()]
}

pub fn parse_dashboard(bytes: &[u8]) -> Result<serde_json::Value, serde_json::Error> {
    serde_json::from_slice(bytes)
}
pub fn parse_sessions(bytes: &[u8]) -> Result<Vec<crate::sessions::Session>, serde_json::Error> {
    serde_json::from_slice(bytes)
}

/// `codexbar dashboard` — one dashboard-v1 snapshot (stdout is only JSON).
pub async fn fetch_dashboard(exe: &str) -> Result<serde_json::Value, String> {
    let stdout = run_cli_read(&dashboard_argv(exe)).await?;
    parse_dashboard(&stdout).map_err(|e| e.to_string())
}

/// `codexbar sessions --json-v2` — live local agent sessions.
pub async fn fetch_sessions(exe: &str) -> Result<Vec<crate::sessions::Session>, String> {
    let stdout = run_cli_read(&sessions_argv(exe)).await?;
    parse_sessions(&stdout).map_err(|e| e.to_string())
}

// --- config mutations ---------------------------------------------------
// All accept `--format json`. API keys and cookies are passed via `--stdin`
// so secrets never appear in argv (visible in `ps`).

pub fn config_set_enabled_argv(exe: &str, provider: &str, enabled: bool) -> Vec<String> {
    vec![
        exe.into(),
        "config".into(),
        if enabled {
            "enable".into()
        } else {
            "disable".into()
        },
        "--provider".into(),
        provider.into(),
        "--format".into(),
        "json".into(),
    ]
}
pub fn config_set_api_key_argv(exe: &str, provider: &str) -> Vec<String> {
    vec![
        exe.into(),
        "config".into(),
        "set-api-key".into(),
        "--provider".into(),
        provider.into(),
        "--stdin".into(),
        "--format".into(),
        "json".into(),
    ]
}
pub fn config_set_cookie_argv(exe: &str, provider: &str) -> Vec<String> {
    vec![
        exe.into(),
        "config".into(),
        "set-cookie".into(),
        "--provider".into(),
        provider.into(),
        "--stdin".into(),
        "--format".into(),
        "json".into(),
    ]
}
pub fn config_clear_credential_argv(exe: &str, provider: &str) -> Vec<String> {
    vec![
        exe.into(),
        "config".into(),
        "clear-credential".into(),
        "--provider".into(),
        provider.into(),
        "--format".into(),
        "json".into(),
    ]
}

fn parse_config_output(stdout: &[u8]) -> Result<serde_json::Value, String> {
    if stdout.iter().all(|b| b.is_ascii_whitespace()) {
        return Ok(serde_json::Value::Null);
    }
    serde_json::from_slice(stdout).map_err(|e| e.to_string())
}

pub async fn config_set_enabled(
    exe: &str,
    provider: &str,
    enabled: bool,
) -> Result<serde_json::Value, String> {
    parse_config_output(&run_cli(&config_set_enabled_argv(exe, provider, enabled), None).await?)
}
pub async fn config_set_api_key(
    exe: &str,
    provider: &str,
    key: &str,
) -> Result<serde_json::Value, String> {
    parse_config_output(&run_cli(&config_set_api_key_argv(exe, provider), Some(key)).await?)
}
pub async fn config_set_cookie(
    exe: &str,
    provider: &str,
    cookie: &str,
) -> Result<serde_json::Value, String> {
    parse_config_output(&run_cli(&config_set_cookie_argv(exe, provider), Some(cookie)).await?)
}
pub async fn config_clear_credential(
    exe: &str,
    provider: &str,
) -> Result<serde_json::Value, String> {
    parse_config_output(&run_cli(&config_clear_credential_argv(exe, provider), None).await?)
}
/// `hooks` section of the shared config, as `codexbar hooks list --format json` prints it.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct HooksInfo {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub events: Vec<HookRuleInfo>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct HookRuleInfo {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub event: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub executable: String,
    #[serde(default)]
    pub arguments: Vec<String>,
}

pub async fn hooks_list(exe: &str) -> Result<HooksInfo, String> {
    let argv: Vec<String> = [exe, "hooks", "list", "--format", "json"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    serde_json::from_slice(&run_cli(&argv, None).await?).map_err(|e| e.to_string())
}

pub async fn hooks_set_enabled(exe: &str, enabled: bool) -> Result<(), String> {
    let verb = if enabled { "enable" } else { "disable" };
    let argv: Vec<String> = [exe, "hooks", verb, "--format", "json"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    run_cli(&argv, None).await.map(|_| ())
}

pub async fn fetch_catalog(exe: &str) -> Vec<ProviderCatalogEntry> {
    let out = tokio::process::Command::new(exe)
        .args(["config", "providers", "--format", "json"])
        .output()
        .await;
    match out {
        Ok(o) if o.status.success() => parse_providers(&o.stdout).unwrap_or_default(),
        _ => [("codex", "Codex"), ("claude", "Claude")]
            .into_iter()
            .map(|(id, name)| ProviderCatalogEntry {
                provider: id.into(),
                display_name: name.into(),
                enabled: true,
                authentication: ProviderAuth::default(),
            })
            .collect(),
    }
}
pub async fn account_action(provider: &str, login: bool) {
    if let Some(bin) = [("codex", "codex"), ("claude", "claude")]
        .iter()
        .find(|(p, _)| *p == provider)
        .map(|(_, b)| *b)
    {
        let action = if login { "login" } else { "logout" };
        let _ = tokio::process::Command::new("xdg-terminal-exec")
            .args(["--", bin, "auth", action])
            .spawn();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn hooks_list_json_parses() {
        let raw = br#"{"events":[{"id":"a","enabled":true,"event":"quota_low","provider":"codex","executable":"/bin/notify","arguments":["x"],"timeoutSeconds":5}],"enabled":true}"#;
        let h: HooksInfo = serde_json::from_slice(raw).unwrap();
        assert!(h.enabled);
        assert_eq!(h.events[0].event, "quota_low");
        assert_eq!(h.events[0].arguments, vec!["x".to_string()]);
        let empty: HooksInfo = serde_json::from_slice(br#"{"events":[],"enabled":false}"#).unwrap();
        assert_eq!(empty, HooksInfo::default());
    }

    use super::*;
    use std::io::Write;
    use std::time::Duration;

    /// Tests that write then exec a script must not overlap: another test's fork can inherit the
    /// still-open write fd and make exec fail with ETXTBSY ("Text file busy").
    fn spawn_serial() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn fake_exe(name: &str, body: &str) -> String {
        let path = std::env::temp_dir().join(format!(
            "codexbar-cli-test-{}-{}.sh",
            name,
            std::process::id()
        ));
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "#!/bin/sh\n{body}").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn cost_day_model_breakdowns_parse_from_fixture() {
        let rows = parse_cost(include_bytes!("../tests/fixtures/cost-both.json")).unwrap();
        let claude = rows.iter().find(|r| r.provider == "claude").unwrap();
        assert_eq!(claude.daily[0].date, "2026-10-16");
        assert_eq!(claude.daily[0].model_breakdowns.len(), 3);
        assert_eq!(
            claude.daily[0].model_breakdowns[0].model_name,
            "claude-opus-5-5"
        );
        assert!((claude.daily[0].model_breakdowns[0].cost - 59.7012828).abs() < 1e-6);
        assert_eq!(
            claude.daily[0].model_breakdowns[0].total_tokens,
            179_470_483
        );
    }

    #[test]
    fn cost_day_without_breakdowns_defaults_empty() {
        let rows = parse_cost(br#"[{"provider":"x","daily":[{"date":"2026-01-01","totalTokens":5,"totalCost":1.5}]}]"#).unwrap();
        assert!(rows[0].daily[0].model_breakdowns.is_empty());
    }

    #[test]
    fn typed_helper_argv_shapes() {
        assert_eq!(dashboard_argv("codexbar"), ["codexbar", "dashboard"]);
        assert_eq!(
            sessions_argv("codexbar"),
            ["codexbar", "sessions", "--json-v2"]
        );
        assert_eq!(
            config_set_enabled_argv("codexbar", "grok", true),
            [
                "codexbar",
                "config",
                "enable",
                "--provider",
                "grok",
                "--format",
                "json"
            ]
        );
        assert_eq!(
            config_set_enabled_argv("codexbar", "grok", false)[2],
            "disable"
        );
        assert_eq!(
            config_clear_credential_argv("codexbar", "claude"),
            [
                "codexbar",
                "config",
                "clear-credential",
                "--provider",
                "claude",
                "--format",
                "json"
            ]
        );
    }

    #[test]
    fn secret_argv_never_contains_secret() {
        let key_argv = config_set_api_key_argv("codexbar", "zai");
        assert!(key_argv.iter().any(|a| a == "--stdin"));
        assert!(!key_argv.iter().any(|a| a.contains("sk-secret")));
        assert_eq!(key_argv[2], "set-api-key");
        let cookie_argv = config_set_cookie_argv("codexbar", "claude");
        assert!(cookie_argv.iter().any(|a| a == "--stdin"));
        assert_eq!(cookie_argv[2], "set-cookie");
    }

    #[test]
    fn stdout_cap_enforced() {
        assert!(check_stdout_cap(&[0u8; 8], 8).is_ok());
        assert!(check_stdout_cap(&[0u8; 9], 8).is_err());
        assert_eq!(MAX_STDOUT_BYTES, 8 * 1024 * 1024);
        assert_eq!(FETCH_TIMEOUT, Duration::from_secs(60));
    }

    #[test]
    fn dashboard_and_sessions_parse() {
        let dash = parse_dashboard(include_bytes!("../tests/fixtures/dashboard.json")).unwrap();
        assert!(dash
            .get("providers")
            .and_then(|p| p.as_array())
            .is_some_and(|p| !p.is_empty()));
        assert_eq!(dash["providers"][0]["id"], "codex");
        assert!(parse_sessions(b"[]").unwrap().is_empty());
        let one = parse_sessions(br#"[{"provider":"codex"}]"#).unwrap();
        assert_eq!(one[0].provider, "codex");
    }

    #[tokio::test]
    async fn run_command_times_out() {
        let _serial = spawn_serial();
        let exe = fake_exe("sleep", "sleep 5");
        let err = run_command(
            &exe,
            &[],
            None,
            Duration::from_millis(100),
            MAX_STDOUT_BYTES,
            false,
        )
        .await
        .unwrap_err();
        assert!(err.contains("timed out"), "{err}");
    }

    /// Opt-in contract check against real CLI output (never committed; may contain account data):
    /// `CODEXBAR_LIVE_USAGE=usage.json CODEXBAR_LIVE_COST=cost.json cargo test live_ -- --ignored`
    #[test]
    #[ignore]
    fn live_cli_payloads_parse() {
        if let Ok(p) = std::env::var("CODEXBAR_LIVE_USAGE") {
            let rows = parse_usage(&std::fs::read(p).unwrap()).unwrap();
            assert!(!rows.is_empty());
        }
        if let Ok(p) = std::env::var("CODEXBAR_LIVE_COST") {
            parse_cost(&std::fs::read(p).unwrap()).unwrap();
        }
        if let Ok(p) = std::env::var("CODEXBAR_LIVE_PROVIDERS") {
            parse_providers(&std::fs::read(p).unwrap()).unwrap();
        }
    }

    #[test]
    fn error_object_and_string_shapes_parse() {
        let rows = parse_usage(
            br#"[{"provider":"openai","error":{"message":"No key","code":1,"kind":"provider"}},{"provider":"x","error":"boom"},{"provider":"y","error":null}]"#,
        )
        .unwrap();
        assert_eq!(rows[0].error.as_deref(), Some("No key"));
        assert_eq!(rows[1].error.as_deref(), Some("boom"));
        assert_eq!(rows[2].error, None);
    }

    #[tokio::test]
    async fn lenient_read_accepts_partial_failure_exit() {
        let _serial = spawn_serial();
        let exe = fake_exe("partial", "printf '[]'; exit 1");
        let t = Duration::from_secs(5);
        assert_eq!(
            run_command(&exe, &[], None, t, 64, true).await.unwrap(),
            b"[]"
        );
        assert!(run_command(&exe, &[], None, t, 64, false).await.is_err());
        let empty = fake_exe("empty", "exit 1");
        assert!(run_command(&empty, &[], None, t, 64, true).await.is_err());
    }

    #[tokio::test]
    async fn run_command_rejects_oversized_stdout() {
        let _serial = spawn_serial();
        let exe = fake_exe("big", "printf '12345678901234567890'");
        let err = run_command(&exe, &[], None, Duration::from_secs(5), 8, false)
            .await
            .unwrap_err();
        assert!(err.contains("too large"), "{err}");
    }

    #[tokio::test]
    async fn ordered_fanout_is_sequential_and_filters() {
        let _serial = spawn_serial();
        let log =
            std::env::temp_dir().join(format!("codexbar-cli-order-{}.log", std::process::id()));
        let _ = std::fs::remove_file(&log);
        let body = format!(
            "p=''; prev=''; for a in \"$@\"; do if [ \"$prev\" = '--provider' ]; then p=$a; fi; prev=$a; done\n\
             echo \"$p\" >> '{log}'\n\
             printf '[{{\"provider\":\"%s\"}},{{\"provider\":\"other\"}}]' \"$p\"",
            log = log.display());
        let exe = fake_exe("ordered", &body);
        let providers = vec!["claude".to_string(), "codex".to_string()];
        let payloads = fetch_payloads_ordered(&exe, &providers, "auto", false)
            .await
            .unwrap();
        assert_eq!(
            payloads
                .iter()
                .map(|p| p.provider.as_str())
                .collect::<Vec<_>>(),
            ["claude", "codex"]
        );
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "claude\ncodex\n");
        let entries = fetch_entries_ordered(&exe, &providers, "auto", false, false).await;
        assert!(entries.is_ok());
    }

    #[tokio::test]
    async fn ordered_fanout_all_failing_is_err_and_empty_is_err() {
        let _serial = spawn_serial();
        let exe = fake_exe("fail", "echo boom >&2; exit 3");
        assert!(
            fetch_payloads_ordered(&exe, &["codex".into()], "auto", false)
                .await
                .is_err()
        );
        assert!(fetch_payloads_ordered(&exe, &[], "auto", false)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn config_set_api_key_sends_secret_via_stdin_only() {
        let _serial = spawn_serial();
        let dir = std::env::temp_dir().join(format!("codexbar-cli-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let argv_file = dir.join("argv.txt");
        let stdin_file = dir.join("stdin.txt");
        let body = format!(
            "printf '%s ' \"$@\" > '{a}'; cat > '{s}'; printf '{{}}'",
            a = argv_file.display(),
            s = stdin_file.display()
        );
        let exe = fake_exe("cfg", &body);
        config_set_api_key(&exe, "zai", "sk-secret-123")
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(&stdin_file).unwrap(),
            "sk-secret-123"
        );
        let argv = std::fs::read_to_string(&argv_file).unwrap();
        assert!(argv.contains("--stdin"), "{argv}");
        assert!(!argv.contains("sk-secret-123"), "{argv}");
    }
}
