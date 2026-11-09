//! Best-effort "focus this agent session's terminal" (mac `TerminalApp` / Warp focus).
//!
//! The session pid is walked up through `/proc/<pid>/stat` parents; each ancestor is tried as
//! the owner of a window, nearest first. Backends: KWin scripting over D-Bus on KDE Wayland,
//! `xdotool` / `wmctrl` on X11 (and XWayland). Other Wayland compositors expose no portable
//! focus API, so that case is a no-op with an explanatory status. Never synthesizes input:
//! activation is a window-manager request (`_NET_ACTIVE_WINDOW` / KWin `activeWindow`).

use std::path::Path;
use std::process::Command;

const MAX_DEPTH: usize = 64;

/// Parent pid from the contents of a `/proc/<pid>/stat` file. The command name sits in
/// parentheses and may itself contain spaces or parentheses, so split at the last `)`.
pub fn parse_ppid(stat: &str) -> Option<i64> {
    let rest = &stat[stat.rfind(')')? + 1..];
    rest.split_whitespace().nth(1)?.parse().ok()
}

/// `pid` followed by its ancestors, nearest first, stopping before pid 1 / 0, on a missing
/// `/proc` entry, on a cycle, or after `MAX_DEPTH` hops. Empty when `pid` itself is gone.
pub fn ancestor_chain(pid: i64, proc_root: &Path) -> Vec<i64> {
    let mut chain = Vec::new();
    let mut cur = pid;
    while cur > 1 && chain.len() < MAX_DEPTH && !chain.contains(&cur) {
        let Ok(stat) = std::fs::read_to_string(proc_root.join(cur.to_string()).join("stat")) else {
            break;
        };
        chain.push(cur);
        match parse_ppid(&stat) {
            Some(p) => cur = p,
            None => break,
        }
    }
    chain
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    KWin,
    X11,
    Unsupported,
}

/// KDE on Wayland -> KWin scripting; any session with an X display -> xdotool/wmctrl;
/// otherwise nothing portable exists.
pub fn pick_backend(desktop: &str, wayland: bool, x_display: bool) -> Backend {
    let kde = desktop.split(':').any(|d| d.eq_ignore_ascii_case("kde"));
    if kde && wayland {
        Backend::KWin
    } else if x_display {
        Backend::X11
    } else {
        Backend::Unsupported
    }
}

fn env_nonempty(k: &str) -> bool {
    std::env::var_os(k).is_some_and(|v| !v.is_empty())
}

/// KWin script (Plasma 5 and 6 API) activating the first window owned by one of `pids`,
/// tried in order. Only integers are interpolated.
pub fn kwin_script(pids: &[i64]) -> String {
    let list = pids
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "var pids=[{list}];\n\
         var wins=(workspace.windowList?workspace.windowList():workspace.clientList());\n\
         for(var i=0;i<pids.length;i++){{for(var j=0;j<wins.length;j++){{\n\
         if(wins[j].pid===pids[i]){{\n\
         if(wins[j].minimized!==undefined)wins[j].minimized=false;\n\
         if(workspace.activeWindow!==undefined)workspace.activeWindow=wins[j];else workspace.activeClient=wins[j];\n\
         i=pids.length;break;}}}}}}\n"
    )
}

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn have(cmd: &str) -> bool {
    Command::new(cmd)
        .arg("--version")
        .output()
        .map(|_| true)
        .unwrap_or(false)
}

/// First window id (decimal) `xdotool` knows for any of `pids`, in order.
fn xdotool_window(pids: &[i64]) -> Option<String> {
    pids.iter().find_map(|p| {
        run("xdotool", &["search", "--pid", &p.to_string()])?
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .map(str::to_string)
    })
}

/// Window id for any of `pids` from `wmctrl -lp` (`0x... desktop pid host title`).
pub fn wmctrl_window(listing: &str, pids: &[i64]) -> Option<String> {
    pids.iter().find_map(|p| {
        listing.lines().find_map(|l| {
            let mut f = l.split_whitespace();
            let id = f.next()?;
            let pid: i64 = f.nth(1)?.parse().ok()?;
            (pid == *p).then(|| id.to_string())
        })
    })
}

fn focus_x11(pids: &[i64]) -> Result<String, String> {
    if have("xdotool") {
        if let Some(win) = xdotool_window(pids) {
            return run("xdotool", &["windowactivate", &win])
                .map(|_| "Focused the terminal window.".to_string())
                .ok_or_else(|| "xdotool could not activate the window.".to_string());
        }
    }
    if have("wmctrl") {
        if let Some(win) = run("wmctrl", &["-lp"]).and_then(|l| wmctrl_window(&l, pids)) {
            return run("wmctrl", &["-ia", &win])
                .map(|_| "Focused the terminal window.".to_string())
                .ok_or_else(|| "wmctrl could not activate the window.".to_string());
        }
    }
    if !have("xdotool") && !have("wmctrl") {
        return Err("Install xdotool or wmctrl to focus terminals on X11.".into());
    }
    Err("No window found for this session's terminal.".into())
}

fn focus_kwin(pids: &[i64]) -> Result<String, String> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .ok_or("XDG_RUNTIME_DIR is not set.")?
        .join("codexbar-linux");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let name = format!("codexbar-focus-{}", std::process::id());
    let file = dir.join(format!("{name}.js"));
    std::fs::write(&file, kwin_script(pids)).map_err(|e| e.to_string())?;
    let file_s = file.to_string_lossy().into_owned();
    let loaded = run(
        "busctl",
        &[
            "--user",
            "call",
            "org.kde.KWin",
            "/Scripting",
            "org.kde.kwin.Scripting",
            "loadScript",
            "ss",
            &file_s,
            &name,
        ],
    );
    let result = match loaded.as_deref().and_then(|o| o.split_whitespace().nth(1)) {
        Some(id) => {
            // Plasma 6 exposes scripts at /Scripting/ScriptN, Plasma 5 at /N.
            let ok = [format!("/Scripting/Script{id}"), format!("/{id}")]
                .iter()
                .any(|path| {
                    run(
                        "busctl",
                        &[
                            "--user",
                            "call",
                            "org.kde.KWin",
                            path,
                            "org.kde.kwin.Script",
                            "run",
                        ],
                    )
                    .is_some()
                });
            if ok {
                Ok("Asked KWin to focus the terminal window.".to_string())
            } else {
                Err("KWin refused to run the focus script.".to_string())
            }
        }
        None => Err("Could not reach KWin scripting over D-Bus.".to_string()),
    };
    let _ = run(
        "busctl",
        &[
            "--user",
            "call",
            "org.kde.KWin",
            "/Scripting",
            "org.kde.kwin.Scripting",
            "unloadScript",
            "s",
            &name,
        ],
    );
    let _ = std::fs::remove_file(&file);
    result
}

/// Blocking; call from `spawn_blocking`. Ok/Err are both user-facing status lines.
pub fn focus_session(pid: i64) -> Result<String, String> {
    let chain = ancestor_chain(pid, Path::new("/proc"));
    if chain.is_empty() {
        return Err(format!(
            "Process {pid} is not running on this machine (remote or finished session)."
        ));
    }
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    match pick_backend(
        &desktop,
        env_nonempty("WAYLAND_DISPLAY"),
        env_nonempty("DISPLAY"),
    ) {
        Backend::KWin => focus_kwin(&chain),
        Backend::X11 => focus_x11(&chain),
        Backend::Unsupported => Err(
            "Focusing windows is not supported on this Wayland compositor (no portable API). \
             Use Open folder, or switch to the terminal manually."
                .into(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fake_proc(tag: &str, procs: &[(i64, &str, i64)]) -> PathBuf {
        let root = std::env::temp_dir().join(format!("codexbar-proc-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (pid, comm, ppid) in procs {
            let d = root.join(pid.to_string());
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(
                d.join("stat"),
                format!("{pid} ({comm}) S {ppid} 1 1 0 -1 0"),
            )
            .unwrap();
        }
        root
    }

    #[test]
    fn parses_ppid_even_with_hostile_comm() {
        assert_eq!(parse_ppid("42 (bash) S 7 42 42 0"), Some(7));
        assert_eq!(parse_ppid("42 (my (weird) name) S 9 1 1"), Some(9));
        assert_eq!(parse_ppid("42 (a b) R 13 1"), Some(13));
        assert_eq!(parse_ppid("garbage"), None);
        assert_eq!(parse_ppid("42 (x) S"), None);
        assert_eq!(parse_ppid("42 (x) S notanumber 1"), None);
    }

    #[test]
    fn walks_ancestors_nearest_first_and_stops_before_init() {
        // claude(300) -> zsh(200) -> foot(100) -> systemd(1)
        let root = fake_proc(
            "walk",
            &[
                (300, "claude", 200),
                (200, "zsh", 100),
                (100, "foot", 1),
                (1, "systemd", 0),
            ],
        );
        assert_eq!(ancestor_chain(300, &root), [300, 200, 100]);
        assert_eq!(ancestor_chain(100, &root), [100]);
        assert!(ancestor_chain(1, &root).is_empty());
        assert!(ancestor_chain(999, &root).is_empty(), "missing pid");
    }

    #[test]
    fn walk_survives_cycles_gaps_and_deep_trees() {
        let cyc = fake_proc("cycle", &[(10, "a", 11), (11, "b", 10)]);
        assert_eq!(ancestor_chain(10, &cyc), [10, 11]);
        // Parent vanished mid-walk: keep what we have.
        let gap = fake_proc("gap", &[(20, "a", 21)]);
        assert_eq!(ancestor_chain(20, &gap), [20]);
        // Very deep chain is capped.
        let procs: Vec<(i64, &str, i64)> = (2..200).map(|p| (p, "n", p + 1)).collect();
        let deep = fake_proc("deep", &procs);
        assert_eq!(ancestor_chain(2, &deep).len(), MAX_DEPTH);
    }

    #[test]
    fn backend_selection() {
        assert_eq!(pick_backend("KDE", true, true), Backend::KWin);
        assert_eq!(pick_backend("ubuntu:GNOME", true, true), Backend::X11);
        assert_eq!(pick_backend("X-Cinnamon", false, true), Backend::X11);
        assert_eq!(pick_backend("COSMIC", true, false), Backend::Unsupported);
        assert_eq!(pick_backend("KDE", false, false), Backend::Unsupported);
    }

    #[test]
    fn kwin_script_embeds_only_integers_in_order() {
        let s = kwin_script(&[300, 200, 100]);
        assert!(s.starts_with("var pids=[300,200,100];"));
        assert!(!s.contains("sendKey") && !s.contains("keyboard"));
        assert!(kwin_script(&[]).starts_with("var pids=[];"));
    }

    #[test]
    fn wmctrl_listing_matches_nearest_ancestor_first() {
        let l = "0x01400003  0 4242  host Terminal\n0x01600003  0 100  host other\n";
        assert_eq!(
            wmctrl_window(l, &[300, 100, 4242]),
            Some("0x01600003".into())
        );
        assert_eq!(wmctrl_window(l, &[7]), None);
        assert_eq!(wmctrl_window("junk line", &[7]), None);
    }
}
