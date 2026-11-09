#!/usr/bin/env python3
"""Install the Rust Linux desktop app for this user.

Thin wrapper over Integrations/Linux/install.py (shared with the Qt app): it
installs ~/.local/bin/codexbar-linux, the launcher, autostart entry, icon and
optionally the Omarchy adapter. All flags are forwarded unchanged
(--binary/--cli/--omarchy/--no-autostart/--provider). When --binary is not
given, a locally built Rust binary (or the tarball's bin/codexbar-linux) is
selected automatically.
"""
import importlib.util
import os
from pathlib import Path
import shutil
import sys

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
DELEGATE = HERE.parent / 'Linux/install.py'


def default_binary():
    candidates = [
        REPO / 'bin/codexbar-linux',  # release tarball layout
        HERE / 'target/release/codexbar-linux',
        HERE / 'target/debug/codexbar-linux',
    ]
    for candidate in candidates:
        if candidate.is_file() and os.access(candidate, os.X_OK):
            return candidate
    return None


def install_cosmic_helper():
    # Optional GPL helper (COSMIC Focus) shipped in the tarball; the app finds it beside itself.
    helper = REPO / 'bin/codexbar-cosmic-focus'
    if not helper.is_file():
        return
    destination = Path.home() / '.local/bin/codexbar-cosmic-focus'
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = destination.with_name(destination.name + '.tmp')
    shutil.copyfile(helper, temporary)
    temporary.chmod(0o755)
    os.replace(temporary, destination)
    print(f'Installed {destination}')


def main(argv=None):
    argv = list(sys.argv[1:] if argv is None else argv)
    if not DELEGATE.is_file():
        raise SystemExit(f'Shared installer not found: {DELEGATE}')
    if not any(arg == '--binary' or arg.startswith('--binary=') for arg in argv):
        binary = default_binary()
        if binary is not None:
            argv += ['--binary', str(binary)]
    spec = importlib.util.spec_from_file_location('codexbar_linux_install', DELEGATE)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    original = sys.argv
    sys.argv = [str(DELEGATE), *argv]
    try:
        module.main()
    finally:
        sys.argv = original
    install_cosmic_helper()


if __name__ == '__main__':
    main()
