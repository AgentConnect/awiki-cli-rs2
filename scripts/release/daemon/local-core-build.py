#!/usr/bin/env python3
"""显式临时 Daemon 构建：隔离已提交 Core 源码，其他 SDK 保持 registry。"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[3]
LOCK = Path('scripts/release/daemon/local-core.Cargo.lock')
spec = importlib.util.spec_from_file_location(
    'registry_build', ROOT / 'scripts/release/registry-build.py')
registry = importlib.util.module_from_spec(spec)
spec.loader.exec_module(registry)


def prepare_manifests(root, versions):
    manifest = root / 'Cargo.toml'
    content = manifest.read_text()
    for name in ('anp', 'anp-identity'):
        content = registry.registry_dependency(content, name, versions[name])
    manifest.write_text(content)


def verify_metadata(metadata, versions, checkout):
    for name, version in versions.items():
        packages = [p for p in metadata['packages'] if p['name'] == name]
        if len(packages) != 1 or packages[0]['version'] != version:
            raise ValueError(f'{name}: missing, duplicate or incompatible SDK version')
        package = packages[0]
        if name == 'awiki-im-core':
            expected = (checkout / 'crates/im-core/Cargo.toml').resolve()
            if package.get('source') is not None or Path(package['manifest_path']).resolve() != expected:
                raise ValueError('Core must resolve from this exact committed workspace')
        elif package.get('source') != 'registry+https://github.com/rust-lang/crates.io-index':
            raise ValueError(f'{name}: must remain a pinned crates.io dependency')


def prepare(root, checkout, versions, cargo, refresh=False):
    if registry.run(['git', 'status', '--porcelain', '--untracked-files=no'],
                    root, capture=True).strip():
        raise ValueError('Commit tracked source changes before a local Core build')
    registry.run(['git', 'worktree', 'add', '--detach', str(checkout), 'HEAD'], root)
    try:
        prepare_manifests(checkout, versions)
        if refresh:
            registry.run([*cargo, 'update', '--workspace'], checkout)
        else:
            shutil.copy2(root / LOCK, checkout / 'Cargo.lock')
        metadata = json.loads(registry.run(
            [*cargo, 'metadata', '--format-version', '1', '--locked'], checkout, capture=True))
        verify_metadata(metadata, versions, checkout)
        if refresh:
            shutil.copy2(checkout / 'Cargo.lock', root / LOCK)
    except BaseException:
        registry.run(['git', 'worktree', 'remove', '--force', str(checkout)], root)
        raise


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--refresh-lock', action='store_true')
    parser.add_argument('--provenance', type=Path)
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args(argv)
    command = args.command[1:] if args.command[:1] == ['--'] else args.command
    if args.refresh_lock and (command or args.provenance):
        parser.error('--refresh-lock cannot be combined with a build')
    if not args.refresh_lock and (not command or not args.provenance):
        parser.error('A build requires --provenance and a Cargo command after --')
    cargo = [os.environ.get('CARGO', 'cargo')]
    if cargo == ['cargo']:
        cargo.append('+' + os.environ.get('AWIKI_CLI_RUST_TOOLCHAIN', '1.88.0'))
    if command:
        cargo = command[:2] if len(command) > 1 and command[1].startswith('+') else command[:1]
    versions = registry.load_versions(ROOT)
    with tempfile.TemporaryDirectory(prefix='awiki-daemon-local-core-') as temporary:
        checkout = Path(temporary) / 'source'
        prepare(ROOT, checkout, versions, cargo, args.refresh_lock)
        try:
            if command:
                env = os.environ.copy()
                target = Path(env.get('CARGO_TARGET_DIR', str(ROOT / 'target')))
                env['CARGO_TARGET_DIR'] = str(target if target.is_absolute() else ROOT / target)
                if '--locked' not in command:
                    command.append('--locked')
                registry.run(command, checkout, env=env)
                commit = registry.run(['git', 'rev-parse', 'HEAD'], checkout, capture=True).strip()
                evidence = {
                    'schema_version': 1,
                    'dependency_mode': 'local-core',
                    'source_commit': commit,
                    'lock_sha256': hashlib.sha256((checkout / 'Cargo.lock').read_bytes()).hexdigest(),
                    'dependencies': {
                        name: {'version': version, 'source': 'workspace' if name == 'awiki-im-core' else 'crates.io'}
                        for name, version in versions.items()
                    },
                    'cargo_version': registry.run([*cargo, '--version'], checkout, capture=True).strip(),
                    'build_command': command,
                }
                args.provenance.parent.mkdir(parents=True, exist_ok=True)
                args.provenance.write_text(json.dumps(evidence, indent=2) + '\n')
        finally:
            registry.run(['git', 'worktree', 'remove', '--force', str(checkout)], ROOT)
    return 0


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        print(f'ERROR: {error}', file=sys.stderr)
        sys.exit(1)
