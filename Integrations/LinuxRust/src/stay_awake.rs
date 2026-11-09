//! Stay Awake (mac `AgentSessionPowerAssertion`): hold a logind idle inhibitor only while the
//! setting is on AND at least one live local agent session exists.
//!
//! Mechanism: `systemd-inhibit --what=idle ... sleep infinity` (argv, no shell); the inhibitor
//! lives as long as that child. `idle` is the right lock: it only stops logind's idle action
//! (the automatic idle suspend), matching mac's `PreventUserIdleSystemSleep`. `sleep` would
//! also block user-initiated suspend and `handle-lid-switch` would block lid-close handling,
//! which the mac feature explicitly does not do.

use crate::sessions::Session;
use std::process::{Child, Command, Stdio};

pub const LABEL: &str = "Stay Awake";
pub const SUBTITLE: &str = "Prevent idle system sleep while a local agent process is running, even when idle. Uses battery power; does not prevent lid-close or display sleep.";

pub fn inhibit_argv() -> Vec<String> {
    [
        "systemd-inhibit",
        "--what=idle",
        "--who=CodexBar",
        "--why=CodexBar: local agent session is live",
        "--mode=block",
        "sleep",
        "infinity",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// mac: a live session is a local one with a positive pid (remote rows carry a host).
pub fn has_live_session(sessions: &[Session]) -> bool {
    sessions
        .iter()
        .any(|s| s.host.is_empty() && s.pid.unwrap_or(0) > 0)
}

/// A held inhibitor. Dropping it must release the lock.
pub trait Hold {
    fn alive(&mut self) -> bool;
}

pub trait Spawner {
    fn acquire(&mut self) -> Result<Box<dyn Hold>, String>;
}

struct ChildHold(Child);

impl Hold for ChildHold {
    fn alive(&mut self) -> bool {
        matches!(self.0.try_wait(), Ok(None))
    }
}

impl Drop for ChildHold {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub struct SystemdSpawner;

impl Spawner for SystemdSpawner {
    fn acquire(&mut self) -> Result<Box<dyn Hold>, String> {
        let argv = inhibit_argv();
        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::process::CommandExt;
            // SAFETY: only an async-signal-safe prctl between fork and exec. If this app dies
            // without running Drop, the kernel still terminates the inhibitor.
            unsafe {
                cmd.pre_exec(|| {
                    libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                    Ok(())
                });
            }
        }
        match cmd.spawn() {
            Ok(c) => Ok(Box::new(ChildHold(c))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err("systemd-inhibit not found; Stay Awake is unavailable.".into())
            }
            Err(e) => Err(format!("Could not start systemd-inhibit: {e}")),
        }
    }
}

pub struct StayAwake {
    spawner: Box<dyn Spawner>,
    held: Option<Box<dyn Hold>>,
    /// Acquire failed for the current "wanted" period; do not retry until it ends.
    failed: bool,
    status: String,
}

impl StayAwake {
    pub fn new() -> Self {
        Self::with_spawner(Box::new(SystemdSpawner))
    }

    pub fn with_spawner(spawner: Box<dyn Spawner>) -> Self {
        Self {
            spawner,
            held: None,
            failed: false,
            status: String::new(),
        }
    }

    #[cfg(test)]
    pub fn is_held(&self) -> bool {
        self.held.is_some()
    }

    /// Line shown under the toggle: empty when there is nothing to say.
    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn release(&mut self) {
        self.held = None;
    }

    /// Reconcile the inhibitor with the desired state. Cheap; call on every session refresh
    /// and settings change.
    pub fn sync(&mut self, enabled: bool, live_session: bool) {
        if self.held.as_mut().is_some_and(|h| !h.alive()) {
            self.held = None; // the inhibitor process died; re-acquire below if still wanted
        }
        let want = enabled && live_session;
        if !want {
            self.release();
            self.failed = false;
            if !enabled {
                self.status.clear();
            }
            return;
        }
        if self.held.is_some() || self.failed {
            return;
        }
        match self.spawner.acquire() {
            Ok(h) => {
                self.held = Some(h);
                self.status = "Holding: a live agent session is keeping the system awake.".into();
            }
            Err(e) => {
                self.failed = true;
                self.status = e;
            }
        }
    }
}

impl Drop for StayAwake {
    fn drop(&mut self) {
        self.release();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Default)]
    struct Log {
        acquired: u32,
        released: u32,
        dead: bool,
        fail: Option<String>,
    }

    struct FakeHold(Rc<RefCell<Log>>);
    impl Hold for FakeHold {
        fn alive(&mut self) -> bool {
            !self.0.borrow().dead
        }
    }
    impl Drop for FakeHold {
        fn drop(&mut self) {
            self.0.borrow_mut().released += 1;
        }
    }
    struct FakeSpawner(Rc<RefCell<Log>>);
    impl Spawner for FakeSpawner {
        fn acquire(&mut self) -> Result<Box<dyn Hold>, String> {
            let mut l = self.0.borrow_mut();
            if let Some(e) = l.fail.clone() {
                return Err(e);
            }
            l.dead = false;
            l.acquired += 1;
            Ok(Box::new(FakeHold(self.0.clone())))
        }
    }

    fn fake() -> (StayAwake, Rc<RefCell<Log>>) {
        let log = Rc::new(RefCell::new(Log::default()));
        (
            StayAwake::with_spawner(Box::new(FakeSpawner(log.clone()))),
            log,
        )
    }

    #[test]
    fn acquires_only_when_enabled_and_live() {
        let (mut s, log) = fake();
        s.sync(false, true);
        s.sync(true, false);
        assert_eq!(log.borrow().acquired, 0);
        s.sync(true, true);
        s.sync(true, true);
        assert_eq!(log.borrow().acquired, 1, "idempotent");
        assert!(s.is_held());
    }

    #[test]
    fn releases_when_session_ends_or_setting_off() {
        let (mut s, log) = fake();
        s.sync(true, true);
        s.sync(true, false);
        assert!(!s.is_held());
        assert_eq!(log.borrow().released, 1);
        s.sync(true, true);
        s.sync(false, true);
        assert_eq!(log.borrow().acquired, 2);
        assert_eq!(log.borrow().released, 2);
        assert_eq!(s.status(), "");
    }

    #[test]
    fn drop_releases() {
        let (mut s, log) = fake();
        s.sync(true, true);
        drop(s);
        assert_eq!(log.borrow().released, 1);
    }

    #[test]
    fn failure_is_quiet_reported_and_not_retried_until_wanted_again() {
        let (mut s, log) = fake();
        log.borrow_mut().fail = Some("systemd-inhibit not found".into());
        s.sync(true, true);
        s.sync(true, true);
        assert!(!s.is_held());
        assert_eq!(s.status(), "systemd-inhibit not found");
        log.borrow_mut().fail = None;
        s.sync(true, true);
        assert!(!s.is_held(), "no retry within the same period");
        s.sync(true, false);
        s.sync(true, true);
        assert!(s.is_held());
    }

    #[test]
    fn reacquires_when_the_inhibitor_process_died() {
        let (mut s, log) = fake();
        s.sync(true, true);
        log.borrow_mut().dead = true;
        s.sync(true, true);
        assert_eq!(log.borrow().acquired, 2);
        assert!(s.is_held());
    }

    #[test]
    fn argv_blocks_idle_only_without_a_shell() {
        let a = inhibit_argv();
        assert_eq!(a[0], "systemd-inhibit");
        assert!(a.contains(&"--what=idle".to_string()));
        assert!(a.contains(&"--mode=block".to_string()));
        assert_eq!(&a[a.len() - 2..], ["sleep", "infinity"]);
    }

    #[test]
    fn live_session_means_local_with_positive_pid() {
        let s = |pid: Option<i64>, host: &str| Session {
            pid,
            host: host.into(),
            ..Default::default()
        };
        assert!(!has_live_session(&[]));
        assert!(!has_live_session(&[s(None, ""), s(Some(0), "")]));
        assert!(!has_live_session(&[s(Some(5), "devbox")]));
        assert!(has_live_session(&[s(Some(5), "devbox"), s(Some(7), "")]));
    }
}
