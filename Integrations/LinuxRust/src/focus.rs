//! Best-effort "focus this agent session's terminal" (mac `TerminalApp` / Warp focus).
//!
//! The session pid is walked up through `/proc/<pid>/stat` parents; each ancestor is tried as
//! the owner of a window, nearest first. Backends: Hyprland (`hyprctl`), Sway (`swaymsg`) and
//! niri (`niri msg`), each detected by its env var; KWin scripting over D-Bus on KDE Wayland;
//! the "Window Calls" extension's D-Bus API on GNOME Wayland (`gdbus`); `xdotool` / `wmctrl`
//! on X11 (and XWayland). COSMIC goes through the separate GPL `codexbar-cosmic-focus`
//! helper, run as a program and never linked. Other Wayland compositors expose no portable
//! focus API, so that case is a no-op with an explanatory status. Never synthesizes input: activation is a
//! window-manager request (`focuswindow` / `focus` / `focus-window` / `_NET_ACTIVE_WINDOW` /
//! KWin `activeWindow` / Window Calls `Activate`).

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
    Gnome,
    X11,
    Cosmic,
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

/// A compositor-specific socket wins; KDE on Wayland -> KWin scripting; GNOME on Wayland ->
/// Window Calls extension; any session with an X display -> xdotool/wmctrl; otherwise nothing
/// portable exists.
pub fn pick_backend(desktop: &str, wayland: bool, x_display: bool, comp: CompositorEnv) -> Backend {
    let kde = desktop.split(':').any(|d| d.eq_ignore_ascii_case("kde"));
    let gnome = desktop.split(':').any(|d| d.eq_ignore_ascii_case("gnome"));
    let cosmic = desktop.split(':').any(|d| d.eq_ignore_ascii_case("cosmic"));
    if comp.hyprland {
        Backend::Hyprland
    } else if comp.sway {
        Backend::Sway
    } else if comp.niri {
        Backend::Niri
    } else if kde && wayland {
        Backend::KWin
    } else if gnome && wayland {
        Backend::Gnome
    } else if cosmic && wayland {
        Backend::Cosmic
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

/// Decode the string out of `gdbus call` output for a single-string reply, `('...',)`. GVariant
/// quotes with `'` (or `"` when the text holds a `'` but no `"`) and escapes `\`, the quote
/// and control characters (`\n`, `\uXXXX`, ...).
pub fn parse_gdbus_string(out: &str) -> Option<String> {
    let rest = out.trim().strip_prefix('(')?;
    let mut chars = rest.chars();
    let quote = chars.next().filter(|c| *c == '\'' || *c == '"')?;
    let mut text = String::new();
    loop {
        let c = chars.next()?;
        if c == quote {
            break;
        }
        if c != '\\' {
            text.push(c);
            continue;
        }
        match chars.next()? {
            'n' => text.push('\n'),
            't' => text.push('\t'),
            'r' => text.push('\r'),
            'a' => text.push('\x07'),
            'b' => text.push('\x08'),
            'f' => text.push('\x0c'),
            'v' => text.push('\x0b'),
            n @ ('u' | 'U') => {
                let len = if n == 'u' { 4 } else { 8 };
                let hex: String = chars.by_ref().take(len).collect();
                text.push(
                    u32::from_str_radix(&hex, 16)
                        .ok()
                        .and_then(char::from_u32)?,
                );
            }
            other => text.push(other),
        }
    }
    // Only the tuple's trailing `,)` may follow the string.
    chars.as_str().trim().starts_with(',').then_some(text)
}

/// Id of the window in a Window Calls `List` JSON array owned by the nearest of `pids`.
pub fn gnome_window(json: &str, pids: &[i64]) -> Option<u32> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let windows = v.as_array()?;
    pids.iter().find_map(|p| {
        windows.iter().find_map(|w| {
            (w.get("pid")?.as_i64()? == *p)
                .then(|| u32::try_from(w.get("id")?.as_u64()?).ok())
                .flatten()
        })
    })
}

fn gdbus_windows(method: &str, arg: Option<&str>) -> Option<String> {
    let method = format!("org.gnome.Shell.Extensions.Windows.{method}");
    let mut args = vec![
        "call",
        "--session",
        "--dest",
        "org.gnome.Shell",
        "--object-path",
        "/org/gnome/Shell/Extensions/Windows",
        "--method",
        &method,
    ];
    args.extend(arg);
    run("gdbus", &args)
}

fn focus_gnome(pids: &[i64]) -> Result<String, String> {
    const NEEDS_EXT: &str = "GNOME needs the Window Calls extension to focus windows.";
    if !have("gdbus") {
        return Err("gdbus was not found; it is needed to focus terminals on GNOME.".into());
    }
    let listing = gdbus_windows("List", None).ok_or(NEEDS_EXT)?;
    let json = parse_gdbus_string(&listing).ok_or(NEEDS_EXT)?;
    let id = gnome_window(&json, pids).ok_or("No window found for this session's terminal.")?;
    gdbus_windows("Activate", Some(&id.to_string()))
        .map(|_| "Focused the terminal window.".to_string())
        .ok_or_else(|| "The Window Calls extension could not activate the window.".to_string())
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

/// Lowercase and drop `-`, `_` and `.` so `com.system76.CosmicTerm` and `cosmic-term` compare equal.
fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '-' | '_' | '.'))
        .flat_map(char::to_lowercase)
        .collect()
}

/// Does a toplevel `app_id` name this process (`comm` or exe basename)? Compares the whole id and
/// its last dot-segment.
fn app_id_matches(app_id: &str, name: &str) -> bool {
    let name = normalize(name);
    !name.is_empty()
        && (normalize(app_id) == name || normalize(app_id.rsplit('.').next().unwrap_or("")) == name)
}

/// COSMIC toplevels carry no pid, so pick by name: the nearest ancestor (`names` is one
/// `[comm, exe basename]` list per ancestor, nearest first) whose name matches some toplevel's
/// `app_id` wins; among several windows of that app prefer one whose title contains a hint
/// (project name / cwd basename), else the first. `toplevels` are `(id, app_id, title)`.
pub fn pick_cosmic_toplevel(
    toplevels: &[(String, String, String)],
    names: &[Vec<String>],
    hints: &[String],
) -> Option<String> {
    for ancestor in names {
        let matching: Vec<_> = toplevels
            .iter()
            .filter(|(_, app_id, _)| ancestor.iter().any(|n| app_id_matches(app_id, n)))
            .collect();
        if matching.is_empty() {
            continue;
        }
        let hit = matching.iter().find(|(_, _, title)| {
            let title = title.to_lowercase();
            hints
                .iter()
                .any(|h| !h.is_empty() && title.contains(&h.to_lowercase()))
        });
        return Some(hit.unwrap_or(&matching[0]).0.clone());
    }
    None
}

/// Parse `codexbar-cosmic-focus list` output into `(id, app_id, title)` rows.
pub fn parse_cosmic_list(json: &str) -> Vec<(String, String, String)> {
    let Ok(serde_json::Value::Array(rows)) = serde_json::from_str(json) else {
        return Vec::new();
    };
    let text = |r: &serde_json::Value, k: &str| match r.get(k) {
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        Some(serde_json::Value::Number(n)) => Some(n.to_string()),
        _ => None,
    };
    rows.iter()
        .filter_map(|r| {
            Some((
                text(r, "id")?,
                text(r, "app_id").unwrap_or_default(),
                text(r, "title").unwrap_or_default(),
            ))
        })
        .collect()
}

const COSMIC_HELPER: &str = "codexbar-cosmic-focus";

/// The helper next to the running executable, else on `PATH`.
fn cosmic_helper() -> Option<std::path::PathBuf> {
    let beside = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join(COSMIC_HELPER)))
        .filter(|c| c.is_file());
    let on_path = || {
        std::env::var_os("PATH").and_then(|p| {
            std::env::split_paths(&p)
                .map(|d| d.join(COSMIC_HELPER))
                .find(|c| c.is_file())
        })
    };
    beside.or_else(on_path)
}

/// `[comm, exe basename]` for each pid in `chain`.
fn process_names(chain: &[i64]) -> Vec<Vec<String>> {
    chain
        .iter()
        .map(|pid| {
            let dir = Path::new("/proc").join(pid.to_string());
            let comm = std::fs::read_to_string(dir.join("comm")).unwrap_or_default();
            let exe = std::fs::read_link(dir.join("exe"))
                .ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_default();
            vec![comm.trim().to_string(), exe]
        })
        .collect()
}

fn focus_cosmic(chain: &[i64], hints: &[String]) -> Result<String, String> {
    let Some(helper) = cosmic_helper() else {
        return Err(
            "Focus on COSMIC needs the codexbar-cosmic-focus helper (GPL, shipped in the \
                    release tarball)."
                .into(),
        );
    };
    let out = Command::new(&helper)
        .arg("list")
        .output()
        .map_err(|e| format!("Could not run codexbar-cosmic-focus: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "codexbar-cosmic-focus list failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let toplevels = parse_cosmic_list(&String::from_utf8_lossy(&out.stdout));
    let Some(id) = pick_cosmic_toplevel(&toplevels, &process_names(chain), hints) else {
        return Err("No COSMIC window matches this session's terminal.".into());
    };
    let out = Command::new(&helper)
        .args(["activate", &id])
        .output()
        .map_err(|e| format!("Could not run codexbar-cosmic-focus: {e}"))?;
    if out.status.success() {
        Ok("Asked COSMIC to focus the terminal window.".into())
    } else {
        Err(format!(
            "codexbar-cosmic-focus activate failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// Blocking; call from `spawn_blocking`. Ok/Err are both user-facing status lines. `hints` are
/// project / cwd names used only to choose between windows of the same app on COSMIC.
pub fn focus_session(pid: i64, hints: &[String]) -> Result<String, String> {
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
        Backend::Gnome => focus_gnome(&chain),
        Backend::X11 => focus_x11(&chain),
        Backend::Cosmic => focus_cosmic(&chain, hints),
        Backend::Unsupported => Err("Focusing windows is not supported on this compositor \
             (supported: Hyprland, Sway, niri, KDE, GNOME with Window Calls, COSMIC, X11). Use \
             Open folder, or switch to the terminal manually."
            .into()),
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
        assert_eq!(
            pick_backend("ubuntu:GNOME", true, true, none),
            Backend::Gnome
        );
        assert_eq!(pick_backend("GNOME", false, true, none), Backend::X11);
        assert_eq!(pick_backend("X-Cinnamon", false, true, none), Backend::X11);
        assert_eq!(pick_backend("COSMIC", true, false, none), Backend::Cosmic);
        assert_eq!(
            pick_backend("COSMIC", false, false, none),
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

    #[test]
    fn gdbus_output_is_unwrapped_and_unescaped() {
        let out = "('[{\"id\":1,\"title\":\"it\\'s\\\\ a\\n\"}]',)\n";
        assert_eq!(
            parse_gdbus_string(out).as_deref(),
            Some("[{\"id\":1,\"title\":\"it's\\ a\n\"}]")
        );
        assert_eq!(parse_gdbus_string("('[]',)").as_deref(), Some("[]"));
        assert_eq!(parse_gdbus_string("(\"it's\",)").as_deref(), Some("it's"));
        assert_eq!(
            parse_gdbus_string("('caf\\u00e9',)").as_deref(),
            Some("café")
        );
        assert_eq!(parse_gdbus_string("('unterminated"), None);
        assert_eq!(parse_gdbus_string("()"), None);
        assert_eq!(parse_gdbus_string("garbage"), None);
    }

    #[test]
    fn gnome_windows_match_nearest_ancestor() {
        let j = r#"[{"id":11,"pid":100,"wm_class":"foot","title":"a","focus":false},
                    {"id":22,"pid":200,"wm_class":"x","title":"b","focus":true},
                    {"id":-1,"pid":300}]"#;
        assert_eq!(gnome_window(j, &[300, 200, 100]), Some(22));
        assert_eq!(gnome_window(j, &[100]), Some(11));
        assert_eq!(gnome_window(j, &[9]), None);
        assert_eq!(gnome_window("nope", &[100]), None);
    }

    #[test]
    fn gnome_end_to_end_from_gdbus_text() {
        let out = r#"('[{"id":5,"pid":77,"title":"vim \'x\'"}]',)"#;
        let json = parse_gdbus_string(out).unwrap();
        assert_eq!(gnome_window(&json, &[77]), Some(5));
    }

    fn tl(id: &str, app_id: &str, title: &str) -> (String, String, String) {
        (id.into(), app_id.into(), title.into())
    }

    fn names(v: &[&[&str]]) -> Vec<Vec<String>> {
        v.iter()
            .map(|a| a.iter().map(|s| s.to_string()).collect())
            .collect()
    }

    #[test]
    fn cosmic_matches_app_id_forms_against_nearest_ancestor() {
        let tls = [
            tl("a", "com.system76.CosmicFiles", "Files"),
            tl("b", "com.system76.CosmicTerm", "~"),
            tl("c", "firefox", "web"),
        ];
        // claude -> zsh -> cosmic-term
        let n = names(&[&["claude", "node"], &["zsh", "zsh"], &["cosmic-term", ""]]);
        assert_eq!(pick_cosmic_toplevel(&tls, &n, &[]), Some("b".into()));
        assert!(app_id_matches("com.system76.CosmicTerm", "cosmic_term"));
        assert!(app_id_matches("Alacritty", "alacritty"));
        assert!(!app_id_matches("firefox", ""));
        assert_eq!(
            pick_cosmic_toplevel(&tls, &names(&[&["bash", "bash"]]), &[]),
            None
        );
        assert_eq!(pick_cosmic_toplevel(&tls, &[], &[]), None);
    }

    #[test]
    fn cosmic_prefers_title_with_hint_else_first() {
        let tls = [
            tl("a", "com.system76.CosmicTerm", "~/other"),
            tl("b", "com.system76.CosmicTerm", "~/Projects/Widget"),
        ];
        let n = names(&[&["cosmic-term", "cosmic-term"]]);
        let hint = vec!["".to_string(), "widget".to_string()];
        assert_eq!(pick_cosmic_toplevel(&tls, &n, &hint), Some("b".into()));
        assert_eq!(pick_cosmic_toplevel(&tls, &n, &[]), Some("a".into()));
        let miss = vec!["nothing".to_string()];
        assert_eq!(pick_cosmic_toplevel(&tls, &n, &miss), Some("a".into()));
    }

    #[test]
    fn cosmic_list_json_parses_and_skips_bad_rows() {
        let j =
            r#"[{"id":"x1","app_id":"foot","title":"t"},{"id":7,"app_id":"a"},{"app_id":"nope"}]"#;
        assert_eq!(
            parse_cosmic_list(j),
            vec![tl("x1", "foot", "t"), tl("7", "a", "")]
        );
        assert!(parse_cosmic_list("garbage").is_empty());
        assert!(parse_cosmic_list("{}").is_empty());
    }
}
