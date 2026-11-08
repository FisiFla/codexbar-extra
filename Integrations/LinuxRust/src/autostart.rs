//! Login autostart via `$XDG_CONFIG_HOME/autostart/com.steipete.CodexBar.desktop`,
//! mirroring `DesktopController::{launchAtLogin,setLaunchAtLogin}` in the Qt app:
//! enabled means the file exists without a `Hidden=true` line; disabling rewrites
//! the entry with `Hidden=true` instead of deleting it.

use std::path::{Path, PathBuf};

const DESKTOP_FILE: &str = "com.steipete.CodexBar.desktop";

pub fn status() -> bool {
    status_at(&desktop_path())
}

pub fn set(enabled: bool) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|_| "Cannot update login startup.".to_string())?;
    set_at(&desktop_path(), enabled, &exe)
}

/// Autostart entry path under the active config home.
pub fn desktop_path() -> PathBuf {
    desktop_path_in(&config_home())
}

/// Autostart entry path under an explicit config home (testable).
pub fn desktop_path_in(config_home: &Path) -> PathBuf {
    config_home.join("autostart").join(DESKTOP_FILE)
}

fn config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        })
}

/// True when the entry exists and has no `Hidden=true` line (Qt regex
/// `(^|\n)Hidden\s*=\s*true(\n|$)`).
pub fn status_at(path: &Path) -> bool {
    let Ok(body) = std::fs::read_to_string(path) else {
        return false;
    };
    !body.lines().any(|line| {
        let Some((key, value)) = line.split_once('=') else {
            return false;
        };
        key.trim() == "Hidden" && value.trim() == "true"
    })
}

/// Escape an executable path for the quoted `Exec="..."` value, like the Qt code:
/// `%` is doubled first, then `\`, `"`, `` ` `` and `$` are backslash-escaped.
pub fn escape_exec(path: &str) -> String {
    let mut out = path.replace('%', "%%");
    // Qt escapes in the order \, ", `, $; doing backslash first keeps the
    // backslashes added for the others intact.
    for ch in ['\\', '"', '`', '$'] {
        out = out.replace(ch, &format!("\\{ch}"));
    }
    out
}

/// Render the `.desktop` body for `exe` (pure, for tests).
pub fn render_entry(exe: &Path, enabled: bool) -> Result<String, String> {
    let raw = exe
        .to_str()
        .ok_or_else(|| "Cannot update login startup.".to_string())?;
    if raw.contains('\n') || raw.contains('\r') {
        return Err("Cannot update login startup.".into());
    }
    Ok(format!(
        "[Desktop Entry]\nType=Application\nName=CodexBar\nExec=\"{}\" --background\nIcon=codexbar\nTerminal=false\nHidden={}\n",
        escape_exec(raw),
        if enabled { "false" } else { "true" }
    ))
}

/// Write the autostart entry at `path` atomically (tmp + rename).
pub fn set_at(path: &Path, enabled: bool, exe: &Path) -> Result<(), String> {
    let content = render_entry(exe, enabled)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| "Cannot update login startup.".to_string())?;
    }
    let tmp = path.with_extension("desktop.tmp");
    std::fs::write(&tmp, &content).map_err(|_| "Cannot update login startup.".to_string())?;
    std::fs::rename(&tmp, path).map_err(|_| "Cannot update login startup.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "codexbar-autostart-test-{}-{}",
            std::process::id(),
            tag
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn autostart_roundtrip() {
        let dir = tmpdir("roundtrip");
        let path = desktop_path_in(&dir);
        assert!(path.ends_with("autostart/com.steipete.CodexBar.desktop"));
        assert!(!status_at(&path), "missing file is disabled");
        let exe = PathBuf::from("/opt/codex bar/codexbar-linux");
        set_at(&path, true, &exe).unwrap();
        assert!(status_at(&path));
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.starts_with("[Desktop Entry]\nType=Application\nName=CodexBar\n"));
        assert!(
            body.contains("Exec=\"/opt/codex bar/codexbar-linux\" --background\n"),
            "body: {body}"
        );
        assert!(body.contains("Icon=codexbar\nTerminal=false\nHidden=false\n"));
        set_at(&path, false, &exe).unwrap();
        assert!(!status_at(&path), "Hidden=true disables like Qt");
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("Hidden=true\n"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn autostart_exec_escaping() {
        assert_eq!(escape_exec("/a/b%c\"$`\\"), "/a/b%%c\\\"\\$\\`\\\\");
        let exe = std::path::Path::new("/bad\npath");
        let dir = tmpdir("escape");
        assert!(set_at(&desktop_path_in(&dir), true, exe).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn autostart_hidden_variants() {
        let dir = tmpdir("hidden");
        let path = desktop_path_in(&dir);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "[Desktop Entry]\nHidden = true\n").unwrap();
        assert!(!status_at(&path));
        std::fs::write(&path, "[Desktop Entry]\nHidden=false\n").unwrap();
        assert!(status_at(&path));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
