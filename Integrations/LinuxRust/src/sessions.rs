//! Agent Sessions model (mac `AgentSession` / `AgentSessionsStore`): typed rows from
//! `codexbar sessions --json-v2`, plus grouping (state, then provider) and relative-time text.
//! Pure and unit-tested; the window lives in `ui/sessions_view.rs`.

use chrono::{DateTime, Utc};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SessionState {
    Active,
    Idle,
    /// Any state this build does not know about; sorted last.
    #[default]
    #[serde(other)]
    Unknown,
}

impl SessionState {
    pub fn label(self) -> &'static str {
        match self {
            SessionState::Active => "Active",
            SessionState::Idle => "Idle",
            SessionState::Unknown => "Other",
        }
    }
}

/// One session row. Every field is optional on the wire; unknown keys are ignored.
#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub provider: String,
    pub dialect: Option<String>,
    pub source: String,
    pub state: SessionState,
    pub pid: Option<i64>,
    pub cwd: Option<String>,
    pub project_name: Option<String>,
    pub session_name: Option<String>,
    pub started_at: Option<String>,
    pub last_activity_at: Option<String>,
    pub transcript_path: Option<String>,
    pub host: String,
}

impl Session {
    pub fn last_activity(&self) -> Option<DateTime<Utc>> {
        parse_time(self.last_activity_at.as_deref())
    }

    /// Project name, else the cwd's last component, else the session name / short id.
    pub fn title(&self) -> String {
        let non_empty = |s: &Option<String>| s.clone().filter(|s| !s.trim().is_empty());
        let from_cwd = || {
            self.cwd
                .as_deref()
                .and_then(|c| c.trim_end_matches('/').rsplit('/').next())
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };
        let title = non_empty(&self.project_name)
            .or_else(from_cwd)
            .or_else(|| non_empty(&self.session_name))
            .unwrap_or_else(|| self.id.chars().take(8).collect());
        redact_emails(&title)
    }

    /// "CLI", "Desktop app", "IDE" (mac `Source`).
    pub fn source_label(&self) -> &'static str {
        match self.source.as_str() {
            "cli" => "CLI",
            "desktopApp" => "Desktop app",
            "ide" => "IDE",
            _ => "Unknown",
        }
    }
}

fn parse_time(s: Option<&str>) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s?.trim())
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

/// Defensive: sessions never carry emails, but nothing user-typed (names, paths) may show one.
pub fn redact_emails(s: &str) -> String {
    s.split(' ')
        .map(|tok| match tok.split_once('@') {
            Some((l, d)) if !l.is_empty() && d.contains('.') && !d.starts_with('.') => {
                "\u{2022}\u{2022}\u{2022}"
            }
            _ => tok,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// "just now", "42s ago", "5m ago", "3h ago", "2d ago"; an em dash when unknown.
pub fn relative_time(then: Option<DateTime<Utc>>, now: DateTime<Utc>) -> String {
    let Some(then) = then else {
        return "\u{2014}".to_string();
    };
    let secs = (now - then).num_seconds();
    match secs {
        s if s < 5 => "just now".into(),
        s if s < 60 => format!("{s}s ago"),
        s if s < 3600 => format!("{}m ago", s / 60),
        s if s < 86_400 => format!("{}h ago", s / 3600),
        s => format!("{}d ago", s / 86_400),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderGroup {
    pub provider: String,
    pub sessions: Vec<Session>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StateGroup {
    pub state: SessionState,
    pub providers: Vec<ProviderGroup>,
}

fn provider_rank(p: &str) -> usize {
    // Mac `AgentSession.Provider` declaration order, then everything else alphabetically.
    ["codex", "claude", "pi"]
        .iter()
        .position(|k| *k == p)
        .unwrap_or(3)
}

/// Active first, then idle, then unknown; inside a state providers in mac order; inside a
/// provider newest activity first (ties by id, so the order is stable between refreshes).
pub fn group(sessions: &[Session]) -> Vec<StateGroup> {
    let mut states: Vec<StateGroup> = Vec::new();
    for state in [
        SessionState::Active,
        SessionState::Idle,
        SessionState::Unknown,
    ] {
        let mut providers: Vec<ProviderGroup> = Vec::new();
        for s in sessions.iter().filter(|s| s.state == state) {
            match providers.iter_mut().find(|g| g.provider == s.provider) {
                Some(g) => g.sessions.push(s.clone()),
                None => providers.push(ProviderGroup {
                    provider: s.provider.clone(),
                    sessions: vec![s.clone()],
                }),
            }
        }
        if providers.is_empty() {
            continue;
        }
        providers.sort_by(|a, b| {
            (provider_rank(&a.provider), &a.provider)
                .cmp(&(provider_rank(&b.provider), &b.provider))
        });
        for g in &mut providers {
            g.sessions.sort_by(|a, b| {
                b.last_activity()
                    .cmp(&a.last_activity())
                    .then_with(|| a.id.cmp(&b.id))
            });
        }
        states.push(StateGroup { state, providers });
    }
    states
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<Session> {
        crate::cli::parse_sessions(include_bytes!("../tests/fixtures/sessions.json")).unwrap()
    }

    #[test]
    fn parses_fixture_tolerantly() {
        let s = fixture();
        assert_eq!(s.len(), 6);
        let first = &s[0];
        assert_eq!(first.provider, "claude");
        assert_eq!(first.pid, Some(4242));
        assert_eq!(first.host, "devbox");
        assert_eq!(first.source_label(), "CLI");
        // Row with unknown state, unknown key and missing fields still parses.
        let odd = s.iter().find(|x| x.id == "odd").unwrap();
        assert_eq!(odd.state, SessionState::Unknown);
        assert!(odd.pid.is_none() && odd.cwd.is_none());
    }

    #[test]
    fn titles_and_redaction() {
        let s = fixture();
        assert_eq!(s[0].title(), "demo_app");
        let idle = s.iter().find(|x| x.id == "idle-1").unwrap();
        assert_eq!(idle.title(), "notes");
        let odd = s.iter().find(|x| x.id == "odd").unwrap();
        assert_eq!(odd.title(), "odd");
        assert_eq!(
            redact_emails("me@example.com work"),
            "\u{2022}\u{2022}\u{2022} work"
        );
        assert_eq!(redact_emails("plain"), "plain");
        let named = s.iter().find(|x| x.id == "mail").unwrap();
        assert!(!named.title().contains('@'));
    }

    #[test]
    fn groups_state_then_provider_then_recency() {
        let g = group(&fixture());
        let states: Vec<_> = g.iter().map(|x| x.state).collect();
        assert_eq!(
            states,
            [
                SessionState::Active,
                SessionState::Idle,
                SessionState::Unknown
            ]
        );
        let active: Vec<_> = g[0].providers.iter().map(|p| p.provider.as_str()).collect();
        assert_eq!(active, ["codex", "claude"]);
        let ids: Vec<_> = g[0].providers[1]
            .sessions
            .iter()
            .map(|s| s.id.as_str())
            .collect();
        assert_eq!(ids, ["c-new", "c-old"]);
        assert_eq!(g[1].providers[0].provider, "claude");
        assert_eq!(g[1].providers[1].provider, "pi");
        assert!(group(&[]).is_empty());
    }

    #[test]
    fn relative_times() {
        let now = parse_time(Some("2026-11-09T12:00:00Z")).unwrap();
        let t = |s| relative_time(parse_time(Some(s)), now);
        assert_eq!(t("2026-11-09T11:59:58Z"), "just now");
        assert_eq!(t("2026-11-09T11:59:18Z"), "42s ago");
        assert_eq!(t("2026-11-09T11:55:00Z"), "5m ago");
        assert_eq!(t("2026-11-09T09:00:00Z"), "3h ago");
        assert_eq!(t("2026-11-07T12:00:00Z"), "2d ago");
        assert_eq!(relative_time(None, now), "\u{2014}");
    }
}
