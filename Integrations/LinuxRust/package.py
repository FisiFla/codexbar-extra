#!/usr/bin/env python3
"""Package the Rust Linux desktop app without bundling credentials or a provider CLI.

Mirrors Integrations/Linux/package.py (Qt) so a Rust tarball installs with the
same layout and installer paths. Builds with cargo unless --binary is given.
"""
import argparse
import gzip
from pathlib import Path
import platform
import re
import subprocess
import sys
import tarfile

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
ARCHITECTURES = {'x86_64': 'x86_64', 'aarch64': 'aarch64', 'arm64': 'aarch64'}


def cargo_version():
    text = (HERE / 'Cargo.toml').read_text(encoding='utf-8')
    match = re.search(r'(?m)^version\s*=\s*"([^"]+)"', text)
    if not match:
        raise SystemExit('Cannot read the version from Cargo.toml; pass --version')
    return match.group(1)


def build_release():
    cargo = ['cargo', 'build', '--release', '--locked',
             '--manifest-path', str(HERE / 'Cargo.toml')]
    print('+ ' + ' '.join(cargo), file=sys.stderr)
    subprocess.run(cargo, check=True)
    return HERE / 'target/release/codexbar-linux'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path,
                        help='Prebuilt codexbar-linux (default: cargo build --release)')
    parser.add_argument('--cosmic-focus', type=Path,
                        help='Prebuilt codexbar-cosmic-focus (GPL helper); adds it and its license')
    parser.add_argument('--version', default=None,
                        help='Version without the v prefix (default: Cargo.toml version)')
    parser.add_argument('--arch', choices=sorted(set(ARCHITECTURES.values())),
                        help='Archive architecture (default: host machine)')
    parser.add_argument('--output', type=Path, default=REPO / '.local/packages')
    args = parser.parse_args()
    version = args.version if args.version is not None else cargo_version()
    if not re.fullmatch(r'[0-9][a-zA-Z0-9._-]{0,79}', version):
        parser.error('Version must start with a digit and contain only letters, numbers, dots, underscores and hyphens')
    binary = args.binary.resolve() if args.binary else build_release()
    if not binary.is_file():
        parser.error('Build the desktop app first, or supply --binary')
    architecture = args.arch or ARCHITECTURES.get(platform.machine())
    if architecture is None:
        parser.error('Supported architectures: x86_64 and aarch64')
    name = f'CodexBarDesktop-v{version}-linux-{architecture}'
    args.output.mkdir(parents=True, exist_ok=True)
    destination = args.output / f'{name}.tar.gz'
    # Explicit allowlist: never archive the checkout, user config, or local CLI resource tree.
    files = {
        'bin/codexbar-linux': binary,
        'README.md': HERE / 'README.md',
        'LICENSE': REPO / 'LICENSE',
        'Integrations/Linux/install.py': REPO / 'Integrations/Linux/install.py',
        'Integrations/Linux/icon.svg': REPO / 'Integrations/Linux/icon.svg',
        'Integrations/LinuxRust/install.py': HERE / 'install.py',
        'Integrations/Omarchy/Panel.qml': REPO / 'Integrations/Omarchy/Panel.qml',
        'Integrations/Omarchy/manifest.json': REPO / 'Integrations/Omarchy/manifest.json',
    }
    executables = {'bin/codexbar-linux'}
    if args.cosmic_focus:
        # Separate GPL-3.0-only program, shipped beside (never linked into) the MIT app.
        files['bin/codexbar-cosmic-focus'] = args.cosmic_focus.resolve()
        files['licenses/codexbar-cosmic-focus/LICENSE'] = HERE / 'cosmic-focus/LICENSE'
        files['licenses/codexbar-cosmic-focus/README.md'] = HERE / 'cosmic-focus/README.md'
        executables.add('bin/codexbar-cosmic-focus')
    # The adapter reads these beside itself; an archive without them falls back to text tags.
    icons = sorted((REPO / 'Sources/CodexBar/Resources').glob('ProviderIcon-*.svg'))
    if not icons:
        parser.error('No ProviderIcon-*.svg files found in Sources/CodexBar/Resources')
    for icon in icons:
        files[f'Integrations/Omarchy/icons/{icon.name}'] = icon
    for relative, source in files.items():
        if not source.is_file():
            parser.error(f'Missing package input for {relative}: {source}')
    # Deterministic archive: sorted entries, uid/gid 0, no owner names, zero mtimes.
    with destination.open('wb') as raw:
        with gzip.GzipFile(filename='', mode='wb', fileobj=raw, mtime=0) as stream:
            with tarfile.open(fileobj=stream, mode='w') as archive:
                for relative in sorted(files):
                    source = files[relative]
                    info = archive.gettarinfo(str(source), arcname=f'{name}/{relative}')
                    info.uid = info.gid = 0
                    info.uname = info.gname = ''
                    info.mtime = 0
                    info.mode = 0o755 if relative in executables else 0o644
                    with source.open('rb') as content:
                        archive.addfile(info, content)
    print(destination)


if __name__ == '__main__':
    main()
