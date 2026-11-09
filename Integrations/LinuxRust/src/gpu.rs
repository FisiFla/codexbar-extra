//! Render on the GPU the compositor composites on.
//!
//! iced asks wgpu for the low-power adapter, which on a hybrid machine is the integrated GPU. When
//! the compositor runs on the discrete GPU (e.g. a display wired to an NVIDIA card), every frame is
//! then copied across GPUs, which shows as stray lines while moving, resizing or scrolling. Before
//! iced starts, this asks the Wayland compositor for its main DRM device (linux-dmabuf v4 default
//! feedback) and sets `WGPU_POWER_PREF` so wgpu picks that GPU. An explicit `WGPU_POWER_PREF` wins;
//! X11, single-GPU machines and compositors without dmabuf v4 are left alone.

use smithay_client_toolkit::reexports::protocols::wp::linux_dmabuf::zv1::client::{
    zwp_linux_dmabuf_feedback_v1::{self, ZwpLinuxDmabufFeedbackV1},
    zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
};
use std::path::Path;
use wayland_client::{
    globals::{registry_queue_init, GlobalListContents},
    protocol::wl_registry,
    Connection, Dispatch, QueueHandle,
};

const NVIDIA: &str = "0x10de";
const INTEL: &str = "0x8086";
const AMD: &str = "0x1002";

/// Must run before any thread is spawned (it sets an environment variable).
pub fn select_compositor_gpu() {
    if std::env::var_os("WGPU_POWER_PREF").is_some() {
        return;
    }
    let Some(dev) = compositor_main_device() else {
        return;
    };
    if let Some(pref) = preference_for(dev, Path::new("/sys")) {
        std::env::set_var("WGPU_POWER_PREF", pref);
    }
}

/// `low` / `high` for wgpu so it lands on the GPU behind `dev`, or `None` when there is nothing to
/// choose (one GPU) or the vendors don't say which GPU is the integrated one.
pub fn preference_for(dev: u64, sys: &Path) -> Option<&'static str> {
    let (major, minor) = split_dev(dev);
    let main = std::fs::canonicalize(sys.join(format!("dev/char/{major}:{minor}/device"))).ok()?;
    let main_vendor = vendor(&main)?;
    let mut others = Vec::new();
    for entry in std::fs::read_dir(sys.join("class/drm")).ok()?.flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with("renderD") {
            continue;
        }
        let Ok(device) = std::fs::canonicalize(entry.path().join("device")) else {
            continue;
        };
        if device != main {
            if let Some(v) = vendor(&device) {
                others.push(v);
            }
        }
    }
    if others.is_empty() {
        return None;
    }
    match main_vendor.as_str() {
        NVIDIA => Some("high"),
        INTEL => Some("low"),
        // An AMD GPU next to Intel is the discrete one; next to NVIDIA it is the APU.
        AMD if others.iter().any(|v| v == INTEL) => Some("high"),
        AMD if others.iter().any(|v| v == NVIDIA) => Some("low"),
        _ => None,
    }
}

fn vendor(device: &Path) -> Option<String> {
    Some(
        std::fs::read_to_string(device.join("vendor"))
            .ok()?
            .trim()
            .to_lowercase(),
    )
}

/// glibc `major()` / `minor()`.
fn split_dev(dev: u64) -> (u64, u64) {
    let major = ((dev >> 32) & 0xffff_f000) | ((dev >> 8) & 0x0fff);
    let minor = ((dev >> 12) & 0xffff_ff00) | (dev & 0xff);
    (major, minor)
}

#[derive(Default)]
struct State {
    main_device: Option<u64>,
    done: bool,
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpLinuxDmabufV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwpLinuxDmabufV1,
        _: <ZwpLinuxDmabufV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpLinuxDmabufFeedbackV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZwpLinuxDmabufFeedbackV1,
        event: zwp_linux_dmabuf_feedback_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwp_linux_dmabuf_feedback_v1::Event::MainDevice { device } => {
                state.main_device = device
                    .get(..8)
                    .and_then(|b| b.try_into().ok())
                    .map(u64::from_ne_bytes);
            }
            zwp_linux_dmabuf_feedback_v1::Event::Done => state.done = true,
            _ => {}
        }
    }
}

fn compositor_main_device() -> Option<u64> {
    std::env::var_os("WAYLAND_DISPLAY")?;
    let conn = Connection::connect_to_env().ok()?;
    let (globals, mut queue) = registry_queue_init::<State>(&conn).ok()?;
    let qh = queue.handle();
    let dmabuf: ZwpLinuxDmabufV1 = globals.bind(&qh, 4..=4, ()).ok()?;
    let feedback = dmabuf.get_default_feedback(&qh, ());
    let mut state = State::default();
    // The compositor answers within a roundtrip or two; never block startup on it.
    for _ in 0..4 {
        if state.done {
            break;
        }
        queue.roundtrip(&mut state).ok()?;
    }
    feedback.destroy();
    dmabuf.destroy();
    state.main_device
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn gpu(sys: &Path, pci: &str, vendor: &str, node: &str, dev: &str) {
        let device = sys.join("devices").join(pci);
        fs::create_dir_all(&device).unwrap();
        fs::write(device.join("vendor"), format!("{vendor}\n")).unwrap();
        let class = sys.join("class/drm").join(node);
        fs::create_dir_all(&class).unwrap();
        std::os::unix::fs::symlink(&device, class.join("device")).unwrap();
        let chr = sys.join("dev/char").join(dev);
        fs::create_dir_all(&chr).unwrap();
        std::os::unix::fs::symlink(&device, chr.join("device")).unwrap();
    }

    fn makedev(major: u64, minor: u64) -> u64 {
        ((major & 0xffff_f000) << 32)
            | ((major & 0x0fff) << 8)
            | ((minor & 0xffff_ff00) << 12)
            | (minor & 0xff)
    }

    fn sys(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("codexbar-gpu-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn split_dev_matches_glibc() {
        assert_eq!(split_dev(makedev(226, 128)), (226, 128));
        assert_eq!(split_dev(makedev(4097, 70000)), (4097, 70000));
    }

    #[test]
    fn nvidia_compositor_next_to_intel_picks_high() {
        let s = sys("nv");
        gpu(&s, "0000:01:00.0", NVIDIA, "renderD128", "226:128");
        gpu(&s, "0000:00:02.0", INTEL, "renderD129", "226:129");
        assert_eq!(preference_for(makedev(226, 128), &s), Some("high"));
        assert_eq!(preference_for(makedev(226, 129), &s), Some("low"));
        let _ = fs::remove_dir_all(&s);
    }

    #[test]
    fn amd_depends_on_its_neighbour() {
        let s = sys("amd-intel");
        gpu(&s, "0000:03:00.0", AMD, "renderD128", "226:128");
        gpu(&s, "0000:00:02.0", INTEL, "renderD129", "226:129");
        assert_eq!(preference_for(makedev(226, 128), &s), Some("high"));
        let _ = fs::remove_dir_all(&s);
        let s = sys("amd-nv");
        gpu(&s, "0000:c5:00.0", AMD, "renderD128", "226:128");
        gpu(&s, "0000:01:00.0", NVIDIA, "renderD129", "226:129");
        assert_eq!(preference_for(makedev(226, 128), &s), Some("low"));
        let _ = fs::remove_dir_all(&s);
    }

    #[test]
    fn single_gpu_or_unknown_device_is_left_alone() {
        let s = sys("one");
        gpu(&s, "0000:01:00.0", NVIDIA, "renderD128", "226:128");
        assert_eq!(preference_for(makedev(226, 128), &s), None);
        assert_eq!(preference_for(makedev(226, 5), &s), None);
        let _ = fs::remove_dir_all(&s);
    }
}
