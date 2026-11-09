//! Best-effort "focus this agent session's terminal" (mac `TerminalApp` / Warp focus).
//!
//! The session pid is walked up through `/proc/<pid>/stat` parents; each ancestor is tried as
//! the owner of a window, nearest first. Backends: Hyprland (`hyprctl`), Sway (`swaymsg`) and
//! niri (`niri msg`), each detected by its env var; KWin scripting over D-Bus on KDE Wayland;
//! `xdotool` / `wmctrl` on X11 (and XWayland). Other Wayland compositors expose no portable
//! focus API, so that case is a no-op with an explanatory status. Never synthesizes input:
//! activation is a window-manager request (`focuswindow` / `focus` / `focus-window` /
//! `_NET_ACTIVE_WINDOW` / KWin `activeWindow`).

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
    Hyprland,
    Sway,
    Niri,
    KWin,
    X11,
    Unsupported,
}

/// Which compositor-specific env vars are set (`HYPRLAND_INSTANCE_SIGNATURE`, `SWAYSOCK`,
/// `NIRI_SOCKET`).
#[derive(Debug, Clone, Copy, Default)]
pub struct CompositorEnv {
    pub hyprland: bool,
    pub sway: bool,
    pub niri: bool,
}

/// A compositor-specific socket wins; KDE on Wayland -> KWin scripting; any session with an X
/// display -> xdotool/wmctrl; otherwise nothing portable exists.
pub fn pick_backend(desktop: &str, wayland: bool, x_display: bool, comp: CompositorEnv) -> Backend {
    let kde = desktop.split(':').any(|d| d.eq_ignore_ascii_case("kde"));
    if comp.hyprland {
        Backend::Hyprland
    } else if comp.sway {
        Backend::Sway
    } else if comp.niri {
        Backend::Niri
    } else if kde && wayland {
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

/// `address` of the first `hyprctl clients -j` entry owned by any of `pids`, in order.
pub fn hypr_window(json: &str, pids: &[i64]) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let clients = v.as_array()?;
    pids.iter().find_map(|p| {
        clients.iter().find_map(|c| {
            let addr = c.get("address")?.as_str()?;
            let ok = c.get("pid")?.as_i64()? == *p
                && addr.starts_with("0x")
                && addr[2..].bytes().all(|b| b.is_ascii_hexdigit());
            ok.then(|| addr.to_string())
        })
    })
}

fn collect_sway_pids(node: &serde_json::Value, out: &mut Vec<i64>) {
    if let Some(p) = node.get("pid").and_then(|p| p.as_i64()) {
        out.push(p);
    }
    for key in ["nodes", "floating_nodes"] {
        if let Some(kids) = node.get(key).and_then(|k| k.as_array()) {
            for k in kids {
                collect_sway_pids(k, out);
            }
        }
    }
}

/// First of `pids` (nearest first) that owns a window in a `swaymsg -t get_tree` document.
pub fn sway_pid(json: &str, pids: &[i64]) -> Option<i64> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let mut owners = Vec::new();
    collect_sway_pids(&v, &mut owners);
    pids.iter().copied().find(|p| owners.contains(p))
}

/// Id of the first `niri msg --json windows` entry owned by any of `pids`, in order.
pub fn niri_window(json: &str, pids: &[i64]) -> Option<u64> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let windows = v.as_array()?;
    pids.iter().find_map(|p| {
        windows.iter().find_map(|w| {
            (w.get("pid")?.as_i64()? == *p)
                .then(|| w.get("id")?.as_u64())
                .flatten()
        })
    })
}

fn focus_hyprland(pids: &[i64]) -> Result<String, String> {
    if !have("hyprctl") {
        return Err("hyprctl was not found; it is needed to focus terminals on Hyprland.".into());
    }
    let addr = run("hyprctl", &["clients", "-j"])
        .and_then(|j| hypr_window(&j, pids))
        .ok_or("No window found for this session's terminal.")?;
    run(
        "hyprctl",
        &["dispatch", "focuswindow", &format!("address:{addr}")],
    )
    .map(|_| "Focused the terminal window.".to_string())
    .ok_or_else(|| "hyprctl could not focus the window.".to_string())
}

fn focus_sway(pids: &[i64]) -> Result<String, String> {
    if !have("swaymsg") {
        return Err("swaymsg was not found; it is needed to focus terminals on Sway.".into());
    }
    let pid = run("swaymsg", &["-t", "get_tree"])
        .and_then(|j| sway_pid(&j, pids))
        .ok_or("No window found for this session's terminal.")?;
    run("swaymsg", &[&format!("[pid={pid}]"), "focus"])
        .map(|_| "Focused the terminal window.".to_string())
        .ok_or_else(|| "swaymsg could not focus the window.".to_string())
}

fn focus_niri(pids: &[i64]) -> Result<String, String> {
    if !have("niri") {
        return Err("niri was not found; it is needed to focus terminals on niri.".into());
    }
    let id = run("niri", &["msg", "--json", "windows"])
        .and_then(|j| niri_window(&j, pids))
        .ok_or("No window found for this session's terminal.")?;
    run(
        "niri",
        &["msg", "action", "focus-window", "--id", &id.to_string()],
    )
    .map(|_| "Focused the terminal window.".to_string())
    .ok_or_else(|| "niri could not focus the window.".to_string())
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
        CompositorEnv {
            hyprland: env_nonempty("HYPRLAND_INSTANCE_SIGNATURE"),
            sway: env_nonempty("SWAYSOCK"),
            niri: env_nonempty("NIRI_SOCKET"),
        },
    ) {
        Backend::Hyprland => focus_hyprland(&chain),
        Backend::Sway => focus_sway(&chain),
        Backend::Niri => focus_niri(&chain),
        Backend::KWin => focus_kwin(&chain),
        Backend::X11 => focus_x11(&chain),
        Backend::Unsupported => Err(
            "Focusing windows is not supported on this compositor (supported: Hyprland, Sway, \
             niri, KDE, X11). Use Open folder, or switch to the terminal manually."
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
        let none = CompositorEnv::default();
        assert_eq!(pick_backend("KDE", true, true, none), Backend::KWin);
        assert_eq!(pick_backend("ubuntu:GNOME", true, true, none), Backend::X11);
        assert_eq!(pick_backend("X-Cinnamon", false, true, none), Backend::X11);
        assert_eq!(
            pick_backend("COSMIC", true, false, none),
            Backend::Unsupported
        );
        assert_eq!(
            pick_backend("KDE", false, false, none),
            Backend::Unsupported
        );
        let c = |hyprland, sway, niri| CompositorEnv {
            hyprland,
            sway,
            niri,
        };
        assert_eq!(
            pick_backend("Hyprland", true, true, c(true, false, false)),
            Backend::Hyprland
        );
        assert_eq!(
            pick_backend("sway", true, true, c(false, true, false)),
            Backend::Sway
        );
        assert_eq!(
            pick_backend("niri", true, false, c(false, false, true)),
            Backend::Niri
        );
    }

    #[test]
    fn hyprland_clients_match_nearest_ancestor_and_reject_odd_addresses() {
        let j = r#"[{"address":"0xaaa1","pid":100,"class":"foot"},
                    {"address":"0xbbb2","pid":4242,"class":"x"},
                    {"address":"zz; rm","pid":7}]"#;
        assert_eq!(hypr_window(j, &[300, 100, 4242]), Some("0xaaa1".into()));
        assert_eq!(hypr_window(j, &[4242]), Some("0xbbb2".into()));
        assert_eq!(hypr_window(j, &[7]), None, "non-hex address is refused");
        assert_eq!(hypr_window(j, &[9]), None);
        assert_eq!(hypr_window("nope", &[100]), None);
    }

    #[test]
    fn sway_tree_walks_nodes_and_floating_nodes() {
        let j = r#"{"id":1,"type":"root","nodes":[{"id":2,"nodes":[
            {"id":3,"pid":100,"nodes":[]}],
            "floating_nodes":[{"id":4,"pid":200}]}]}"#;
        assert_eq!(sway_pid(j, &[300, 200, 100]), Some(200));
        assert_eq!(sway_pid(j, &[100]), Some(100));
        assert_eq!(sway_pid(j, &[9]), None);
        assert_eq!(sway_pid("[", &[100]), None);
    }

    #[test]
    fn niri_windows_yield_the_id_of_the_nearest_ancestor() {
        let j = r#"[{"id":12,"pid":100,"title":"a"},{"id":15,"pid":200,"title":"b"},
                    {"id":16,"pid":null}]"#;
        assert_eq!(niri_window(j, &[300, 200, 100]), Some(15));
        assert_eq!(niri_window(j, &[100]), Some(12));
        assert_eq!(niri_window(j, &[9]), None);
        assert_eq!(niri_window("{}", &[100]), None);
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
