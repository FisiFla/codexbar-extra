//! Linux-only extras that have no key in `linux.json` (the legacy Qt app rejects unknown keys
//! there), stored in their own `~/.config/codexbar/linux-extras.json`: versioned, written
//! atomically (temp file + rename). Defaults mirror the macOS app (all off).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Extras {
    pub version: u32,
    /// mac `quotaWarningOnScreenAlertEnabled` (default false).
    pub quota_warning_overlay: bool,
    /// mac `confettiOnSessionLimitResetsEnabled` (default false).
    pub confetti_on_session_reset: bool,
    /// mac `confettiOnWeeklyLimitResetsEnabled` (default false).
    pub confetti_on_weekly_reset: bool,
    /// mac `stayAwakeEnabled` (default false): hold an idle inhibitor while an agent runs.
    pub stay_awake: bool,
}

impl Default for Extras {
    fn default() -> Self {
        Self {
            version: VERSION,
            quota_warning_overlay: false,
            confetti_on_session_reset: false,
            confetti_on_weekly_reset: false,
            stay_awake: false,
        }
    }
}

pub fn path() -> PathBuf {
    let mut p = crate::settings::Settings::path();
    p.set_file_name("linux-extras.json");
    p
}

pub fn load() -> Extras {
    load_from(&path())
}

/// Missing, unparseable or newer-version files yield the defaults (features stay off).
pub fn load_from(path: &Path) -> Extras {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice::<Extras>(&b).ok())
        .filter(|e| e.version == VERSION)
        .unwrap_or_default()
}

pub fn save(e: &Extras) -> Result<(), String> {
    save_to(e, &path())
}

pub fn save_to(e: &Extras, path: &Path) -> Result<(), String> {
    // Never clobber a file we cannot understand (corrupt, or written by a newer build).
    if let Ok(existing) = std::fs::read(path) {
        let ok = serde_json::from_slice::<Extras>(&existing)
            .map(|x| x.version <= VERSION)
            .unwrap_or(false);
        if !ok {
            return Err(
                "Unreadable or newer linux-extras.json. Fix or remove it before saving.".into(),
            );
        }
    }
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    let body = serde_json::to_string_pretty(&Extras {
        version: VERSION,
        ..e.clone()
    })
    .map_err(|e| e.to_string())?;
    std::fs::write(&tmp, body).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("codexbar-extras-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn defaults_match_mac_all_off() {
        let e = Extras::default();
        assert!(!e.quota_warning_overlay && !e.confetti_on_session_reset);
        assert!(!e.confetti_on_weekly_reset && !e.stay_awake);
        assert_eq!(e.version, 1);
        assert_eq!(load_from(&dir("missing").join("x.json")), e);
    }

    #[test]
    fn round_trip_is_atomic_and_versioned() {
        let d = dir("rt");
        let p = d.join("linux-extras.json");
        let e = Extras {
            quota_warning_overlay: true,
            confetti_on_weekly_reset: true,
            ..Default::default()
        };
        save_to(&e, &p).unwrap();
        assert_eq!(load_from(&p), e);
        assert!(
            !p.with_extension("json.tmp").exists(),
            "temp file renamed away"
        );
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
        assert_eq!(v["version"], 1);
        assert_eq!(v["quotaWarningOverlay"], true);
        save_to(&Extras::default(), &p).unwrap();
        assert_eq!(load_from(&p), Extras::default());
    }

    #[test]
    fn partial_unknown_version_and_corrupt_fail_closed() {
        let d = dir("bad");
        let p = d.join("e.json");
        std::fs::write(&p, r#"{"version":1,"confettiOnSessionReset":true}"#).unwrap();
        let e = load_from(&p);
        assert!(e.confetti_on_session_reset && !e.quota_warning_overlay);
        // Newer version: ignored on load (features off) and never overwritten.
        std::fs::write(&p, r#"{"version":2,"quotaWarningOverlay":true}"#).unwrap();
        assert_eq!(load_from(&p), Extras::default());
        assert!(save_to(&Extras::default(), &p).is_err());
        std::fs::write(&p, "{nope").unwrap();
        assert_eq!(load_from(&p), Extras::default());
        assert!(save_to(&Extras::default(), &p).is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "{nope");
    }

    #[test]
    fn path_is_a_sibling_of_linux_json() {
        let p = path();
        assert_eq!(p.file_name().unwrap(), "linux-extras.json");
        assert_eq!(p.parent(), crate::settings::Settings::path().parent());
    }
}
