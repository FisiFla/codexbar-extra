use std::path::PathBuf;
use std::sync::{Arc, Mutex};
pub fn runtime_dir() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/tmp/codexbar-{}", unsafe { libc::getuid() })));
    base.join("codexbar-linux")
}
pub fn socket_path() -> PathBuf {
    runtime_dir().join("desktop.sock")
}
pub fn command_from_args(args: &[String]) -> serde_json::Value {
    let has = |n: &str| args.iter().any(|a| a == n);
    let cmd = if has("--snapshot") {
        "snapshot"
    } else if has("--refresh") {
        "refresh"
    } else if has("--settings") {
        "settings"
    } else if has("--sessions") {
        "sessions"
    } else if has("--spending") {
        "spending"
    } else if has("--quick-view") {
        "quick-view"
    } else if has("--quit") {
        "quit"
    } else if has("--background") {
        "background"
    } else if has("--autostart") {
        "autostart"
    } else if has("--configure") {
        "configure"
    } else {
        "usage"
    };
    let mut v = serde_json::json!({"command":cmd});
    if cmd == "snapshot" && has("--with-spending") {
        v["spending"] = serde_json::json!(true)
    }
    if cmd == "autostart" {
        if let Some(i) = args.iter().position(|a| a == "--autostart") {
            v["action"] =
                serde_json::json!(args.get(i + 1).cloned().unwrap_or_else(|| "status".into()));
        }
    }
    if cmd == "configure" {
        if let Some(i) = args.iter().position(|a| a == "--configure") {
            if let Ok(s) = serde_json::from_str::<serde_json::Value>(
                args.get(i + 1).map(|s| s.as_str()).unwrap_or("{}"),
            ) {
                v["settings"] = s;
            }
        }
    }
    v
}
pub fn try_request(msg: &serde_json::Value) -> Option<String> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    let mut s = UnixStream::connect(socket_path()).ok()?;
    s.set_read_timeout(Some(std::time::Duration::from_secs(3)))
        .ok()?;
    writeln!(s, "{msg}").ok()?;
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line).ok()?;
    if line.is_empty() {
        None
    } else {
        Some(line)
    }
}
pub fn snapshot_json(s: &crate::state::Shared) -> serde_json::Value {
    crate::ipc_snapshot::snapshot(s)
}
pub fn stream(
    shared: Arc<Mutex<crate::state::Shared>>,
) -> iced::futures::stream::BoxStream<'static, crate::state::IpcCommand> {
    crate::ipc_snapshot::stream(shared)
}
