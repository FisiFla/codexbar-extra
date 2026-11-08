//! `~/.config/codexbar/linux.json` — drop-in compatible with the Qt app's 24 keys
//! (`Integrations/Linux/DesktopController.cpp` defaults + validation).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Reserved provider selectors (Qt `DesktopController::validate`): valid as
/// `provider`, never as entries of `providerOrder`.
const RESERVED_PROVIDERS: [&str; 4] = ["all", "both", "enabled", "custom"];

/// Equivalent of the Qt regex `^[a-z0-9-]{1,80}$` (no regex crate needed).
fn is_provider_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub executable: String,
    pub provider: String,
    pub source: String,
    pub refresh_seconds: u32,
    pub account_index: u32,
    pub notify_threshold: u32,
    pub all_accounts: bool,
    pub show_identity: bool,
    pub show_costs: bool,
    pub show_status: bool,
    pub notifications: bool,
    pub show_tray: bool,
    pub compact_quick_view: bool,
    pub refresh_on_open: bool,
    pub provider_order: Vec<String>,
    pub quota_display: String,
    pub reset_display: String,
    pub show_pace: bool,
    pub show_bar_detail: bool,
    pub show_scoped_caps: bool,
    pub bar_providers: u32,
    pub warning_colors: bool,
    pub tray_style: String,
    pub follow_omarchy_theme: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            executable: "codexbar".into(),
            provider: "codex".into(),
            source: "auto".into(),
            refresh_seconds: 300,
            account_index: 0,
            notify_threshold: 10,
            all_accounts: false,
            show_identity: false,
            show_costs: true,
            show_status: true,
            notifications: false,
            show_tray: true,
            compact_quick_view: false,
            refresh_on_open: false,
            provider_order: Vec::new(),
            quota_display: "remaining".into(),
            reset_display: "countdown".into(),
            show_pace: true,
            show_bar_detail: false,
            show_scoped_caps: false,
            bar_providers: 2,
            warning_colors: true,
            tray_style: "meters".into(),
            follow_omarchy_theme: false,
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        // The reserved selectors are covered by the same ID shape; naming them
        // keeps the Qt contract explicit.
        if !is_provider_id(&self.provider) && !RESERVED_PROVIDERS.contains(&self.provider.as_str())
        {
            return Err("Enter a provider ID such as codex, claude, both, or enabled.".into());
        }
        if self.executable.trim().is_empty() {
            return Err("Specify the CodexBar CLI executable.".into());
        }
        if self.provider_order.len() > 80
            || (self.provider == "custom" && self.provider_order.is_empty())
        {
            return Err("Choose at least one provider (maximum 80).".into());
        }
        let mut seen = std::collections::HashSet::new();
        for id in &self.provider_order {
            if !is_provider_id(id)
                || RESERVED_PROVIDERS.contains(&id.as_str())
                || !seen.insert(id.as_str())
            {
                return Err("The provider list contains an invalid or duplicate ID.".into());
            }
        }
        if !(60..=3600).contains(&self.refresh_seconds) {
            return Err("refreshSeconds out of range".into());
        }
        if !(1..=99).contains(&self.notify_threshold) {
            return Err("notifyThreshold out of range".into());
        }
        if self.account_index > 999 {
            return Err("accountIndex out of range".into());
        }
        if self.bar_providers > 80 {
            return Err("barProviders out of range".into());
        }
        if !["auto", "oauth", "cli", "api", "web"].contains(&self.source.as_str()) {
            return Err("invalid source".into());
        }
        if !["remaining", "used"].contains(&self.quota_display.as_str()) {
            return Err("invalid quotaDisplay".into());
        }
        if !["countdown", "absolute", "both"].contains(&self.reset_display.as_str()) {
            return Err("invalid resetDisplay".into());
        }
        if !["meters", "icon"].contains(&self.tray_style.as_str()) {
            return Err("invalid trayStyle".into());
        }
        Ok(())
    }

    pub fn path() -> PathBuf {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
            });
        base.join("codexbar").join("linux.json")
    }
}

pub fn load() -> Settings {
    load_from(&Settings::path())
}

/// Missing keys merge with defaults (`#[serde(default)]`); a missing or
/// unparseable file yields defaults, like the Qt app. Unknown keys are
/// ignored on load so newer/older builds can share the file.
pub fn load_from(path: &Path) -> Settings {
    match std::fs::read(path) {
        Ok(b) => serde_json::from_slice(&b).unwrap_or_default(),
        Err(_) => Settings::default(),
    }
}

pub fn save(s: &Settings) -> Result<(), String> {
    save_to(s, &Settings::path())
}

pub fn save_to(s: &Settings, path: &Path) -> Result<(), String> {
    s.validate()?;
    // Qt blocks saving while the on-disk file is invalid, so a corrupt
    // linux.json is never silently overwritten and lost.
    if path.exists() {
        let existing = std::fs::read(path).map_err(|_| {
            "Cannot read linux.json. Check permissions and restart CodexBar.".to_string()
        })?;
        let parseable = serde_json::from_slice::<serde_json::Value>(&existing)
            .map(|v| v.is_object())
            .unwrap_or(false);
        if !parseable {
            return Err(
                "Invalid linux.json. Fix or remove it and restart CodexBar before saving.".into(),
            );
        }
    }
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(
        &tmp,
        serde_json::to_string_pretty(s).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "codexbar-settings-test-{}-{}",
            std::process::id(),
            tag
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn settings_partial_json_merges_defaults() {
        let s: Settings =
            serde_json::from_str(r#"{"provider":"claude","refreshSeconds":120}"#).unwrap();
        assert_eq!(s.provider, "claude");
        assert_eq!(s.refresh_seconds, 120);
        let d = Settings::default();
        assert_eq!(s.executable, d.executable);
        assert_eq!(s.source, d.source);
        assert_eq!(s.notify_threshold, d.notify_threshold);
        assert_eq!(s.show_costs, d.show_costs);
        assert_eq!(s.bar_providers, d.bar_providers);
        assert_eq!(s.tray_style, d.tray_style);
    }

    #[test]
    fn settings_load_missing_file_returns_defaults() {
        let dir = temp_dir("missing");
        let s = load_from(&dir.join("linux.json"));
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn settings_load_partial_file_merges_defaults() {
        let dir = temp_dir("partial");
        let path = dir.join("linux.json");
        std::fs::write(&path, r#"{"provider":"both","showTray":false}"#).unwrap();
        let s = load_from(&path);
        assert_eq!(s.provider, "both");
        assert!(!s.show_tray);
        assert_eq!(s.refresh_seconds, 300);
    }

    #[test]
    fn settings_load_invalid_json_returns_defaults() {
        let dir = temp_dir("invalid");
        let path = dir.join("linux.json");
        std::fs::write(&path, "{not json").unwrap();
        assert_eq!(load_from(&path), Settings::default());
    }

    #[test]
    fn settings_save_refuses_to_overwrite_unparseable_file() {
        let dir = temp_dir("refuse");
        let path = dir.join("linux.json");
        std::fs::write(&path, "{not json").unwrap();
        let err = save_to(&Settings::default(), &path).unwrap_err();
        assert!(err.contains("Fix or remove"), "got: {err}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{not json");
    }

    #[test]
    fn settings_save_roundtrip_overwrites_parseable_file() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("linux.json");
        let mut s = Settings::default();
        s.provider = "claude".into();
        save_to(&s, &path).unwrap();
        assert_eq!(load_from(&path), s);
        // A second save over the parseable file we just wrote must succeed.
        let mut s2 = s.clone();
        s2.refresh_seconds = 600;
        save_to(&s2, &path).unwrap();
        assert_eq!(load_from(&path).refresh_seconds, 600);
    }

    #[test]
    fn settings_validate_provider_ids() {
        for ok in [
            "codex",
            "claude",
            "both",
            "all",
            "enabled",
            "custom",
            "a",
            "deep-seek1",
        ] {
            let mut s = Settings::default();
            s.provider = ok.into();
            if ok == "custom" {
                s.provider_order = vec!["codex".into()];
            }
            assert!(s.validate().is_ok(), "provider {ok:?} should pass");
        }
        for bad in [
            "",
            "Codex",
            "codex!",
            "code x",
            "codex_1",
            "é",
            &"a".repeat(81),
        ] {
            let mut s = Settings::default();
            s.provider = bad.into();
            assert!(s.validate().is_err(), "provider {bad:?} should fail");
        }
    }

    #[test]
    fn settings_validate_provider_order_rules() {
        // Reserved IDs and duplicates are not allowed inside providerOrder.
        for order in [
            vec!["all"],
            vec!["both"],
            vec!["enabled"],
            vec!["custom"],
            vec!["codex", "codex"],
            vec!["Bad!"],
        ] {
            let mut s = Settings::default();
            s.provider_order = order.iter().map(|v| v.to_string()).collect();
            assert!(s.validate().is_err(), "order {order:?} should fail");
        }
        // provider=custom requires a non-empty order.
        let mut s = Settings::default();
        s.provider = "custom".into();
        assert!(s.validate().is_err());
        s.provider_order = vec!["codex".into(), "claude".into()];
        assert!(s.validate().is_ok());
        // More than 80 entries fail.
        let mut s = Settings::default();
        s.provider_order = (0..81).map(|i| format!("p{i}")).collect();
        assert!(s.validate().is_err());
    }

    #[test]
    fn settings_validate_executable_required() {
        let mut s = Settings::default();
        s.executable = "  ".into();
        assert!(s.validate().is_err());
    }
}
