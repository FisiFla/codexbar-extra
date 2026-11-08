# CodexBar for Linux (Rust)

A native Rust desktop app (`codexbar-linux`) with a system tray icon, compact
Quick View, Usage & Spend dashboard and Settings windows, plus the Omarchy bar
adapter. It is a drop-in replacement for the Qt app in `Integrations/Linux/`:
same binary name, command-line flags, IPC socket, settings file and tarball
layout, so existing installs and the Omarchy `Panel.qml` keep working.

The Swift `codexbar` CLI owns provider fetching and authentication. This app
owns polling, settings, notifications and a private local socket for desktop
adapters (`$XDG_RUNTIME_DIR/codexbar-linux/desktop.sock`). No HTTP server is
needed. Settings live in `~/.config/codexbar/linux.json` and stay compatible
with the Qt app (same 24 keys and validation).

## Requirements

- An installed CodexBar Linux CLI (`codexbar`). Install a CodexBarCLI release
  archive (keeping `VERSION` and `CodexBar_CodexBarCore.bundle` beside the
  executable), or use Homebrew / the AUR `codexbar-cli` package, then
  authenticate with your provider's CLI. Without the CLI the desktop has no
  data to show; unknown values render as `—`, never as `0`.
- Runtime: a Wayland or X11 session with a StatusNotifier tray host for the
  tray icon. Release binaries require glibc 2.39+ (Ubuntu 24.04, Debian 13,
  current Arch/Fedora). No Qt runtime is needed.
- Build: stable Rust (rustup), a C linker, `pkg-config`, Python 3 for the
  installer/packager. On Ubuntu 24.04:

  ```sh
  sudo apt update
  sudo apt install build-essential pkg-config python3 libdbus-1-dev
  ```

## Download

Prebuilt x86_64 and aarch64 tarballs are attached to the `linux-v*` releases at
<https://github.com/FisiFla/codexbar-extra/releases> (the macOS app keeps its own
`v*` releases). Each archive has a `.sha256` beside it.

## Build and run (from a checkout)

From the repository root:

```sh
cargo build --release --manifest-path Integrations/LinuxRust/Cargo.toml
./Integrations/LinuxRust/target/release/codexbar-linux --settings
```

Useful flags (identical to the Qt app): `--usage`, `--quick-view`,
`--settings`, `--spending`, `--background`, `--no-tray`, `--cli PATH`,
`--snapshot [--with-spending]`, `--refresh`, `--quit`,
`--autostart status|enable|disable`, `--configure JSON`, `--version`.

Run the tests with:

```sh
cargo test --manifest-path Integrations/LinuxRust/Cargo.toml
```

## Install

From a checkout (builds are picked up from `target/release` automatically):

```sh
python3 Integrations/LinuxRust/install.py --cli /absolute/path/to/codexbar
~/.local/bin/codexbar-linux --settings
```

From a release tarball (`CodexBarDesktop-v<version>-linux-<arch>.tar.gz`,
verify its `.sha256` first with `sha256sum -c`):

```sh
tar -xzf CodexBarDesktop-v<version>-linux-<arch>.tar.gz
cd CodexBarDesktop-v<version>-linux-<arch>
python3 Integrations/LinuxRust/install.py --cli /absolute/path/to/codexbar
```

The installer is per-user and shared with the Qt app: it writes
`~/.local/bin/codexbar-linux`, the `com.steipete.CodexBar` launcher and
autostart entries and the `codexbar` icon, merges your existing
`linux.json` (backing it up first) and honors absolute `XDG_CONFIG_HOME` /
`XDG_DATA_HOME`. Options: `--binary PATH` (prebuilt binary), `--provider ID`
(initial provider), `--no-autostart` (do not start at login) and `--omarchy`
(also install the Omarchy bar adapter into
`~/.config/omarchy/plugins/steipete.codexbar`, register it in `shell.json`
with a backup, and hide the tray icon in favor of the bar widget).

To package a tarball yourself:

```sh
python3 Integrations/LinuxRust/package.py            # cargo build --release, then package
python3 Integrations/LinuxRust/package.py --binary /path/to/codexbar-linux --version 1.2.3
```

The archive lands in `.local/packages/`; create its checksum with
`Scripts/generate_release_checksum.sh <archive>`.

## Uninstall

Quit CodexBar (`codexbar-linux --quit`), then remove
`~/.local/bin/codexbar-linux`,
`$XDG_DATA_HOME/applications/com.steipete.CodexBar.desktop`,
`$XDG_DATA_HOME/icons/hicolor/scalable/apps/codexbar.svg` and
`$XDG_CONFIG_HOME/autostart/com.steipete.CodexBar.desktop` (default data and
config directories are `~/.local/share` and `~/.config`). On Omarchy, also
remove `~/.config/omarchy/plugins/steipete.codexbar` and its `shell.json`
layout entry (restoring the `shell.json.codexbar-backup-*` file works too).
Preferences in `~/.config/codexbar/` and their backups can be kept for a
later reinstall or deleted.

## Parity notes

- Same contracts as the Qt app: binary/argv, IPC socket and snapshot
  `schemaVersion: 1` (no email addresses over IPC), `linux.json` keys and
  validation (including refusing to save over an invalid `linux.json`),
  tarball name/layout and the Omarchy plugin ID `steipete.codexbar`.
- The GUI is `iced` (wgpu) instead of Qt Quick and the tray uses
  StatusNotifierItem over D-Bus; there is no Qt runtime dependency.
- Data comes only from the `codexbar` CLI (`usage`, `cost`,
  `config providers` JSON). Provider fetching, sign-in and costs stay in the
  CLI; features that are macOS-only upstream (browser-cookie auto-import,
  iCloud sync, Sparkle updates, widgets) are not available on Linux.
