#!/usr/bin/env python3
"""Build release consumers from pinned registry SDKs in an isolated Git worktree."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
CONFIG = Path('scripts/release/registry-dependencies.json')
LOCK = Path('scripts/release/registry-Cargo.lock')
CONSUMERS = ('awiki-cli', 'awiki-deamon', 'im-core-dart', 'im-core-node')


def run(command, cwd, *, capture=False, env=None):
    return subprocess.run(command, cwd=cwd, check=True, env=env, text=True,
                          stdout=subprocess.PIPE if capture else None).stdout


def load_versions(root):
    config = json.loads((root / CONFIG).read_text(encoding='utf-8'))
    versions = config.get('packages', {})
    if (config.get('schema_version') != 1 or config.get('registry') != 'crates-io'
            or set(versions) != {'anp', 'anp-identity', 'awiki-im-core'}
            or any(not re.fullmatch(r'\d+\.\d+\.\d+', value) for value in versions.values())):
        raise ValueError('Invalid registry-dependencies.json')
    return versions


def registry_dependency(text, key, version):
    pattern = rf'(?m)^({re.escape(key)}\s*=\s*\{{)([^\n}}]*)(\}})'
    def replace(match):
        fields = match[2]
        # These owner manifests intentionally use single-line dependency tables.
        fields = re.sub(r'\b(?:path|version)\s*=\s*"[^"]*"\s*,?\s*', '', fields)
        fields = fields.strip().strip(',').strip()
        return match[1] + f' version = "={version}"' + (', ' + fields if fields else '') + ' }'
    result, count = re.subn(pattern, replace, text)
    if count != 1:
        raise ValueError(f'Expected one inline dependency for {key}, found {count}')
    return result


def prepare_manifests(root, versions):
    manifest = root / 'Cargo.toml'
    content = manifest.read_text(encoding='utf-8')
    content, count = re.subn(r'(?m)^\s*"crates/im-core",\s*\n', '', content)
    if count != 1:
        raise ValueError('Expected the local im-core workspace member')
    for name in ('anp', 'anp-identity'):
        content = registry_dependency(content, name, versions[name])
    manifest.write_text(content, encoding='utf-8')
    for consumer in CONSUMERS:
        manifest = root / 'crates' / consumer / 'Cargo.toml'
        content = registry_dependency(manifest.read_text(encoding='utf-8'),
                                      'im-core', versions['awiki-im-core'])
        manifest.write_text(content, encoding='utf-8')


def verify_metadata(metadata, versions):
    for name, version in versions.items():
        packages = [p for p in metadata['packages'] if p['name'] == name]
        if not packages or any(p['version'] != version or
                p.get('source') != 'registry+https://github.com/rust-lang/crates.io-index'
                for p in packages):
            raise ValueError(f'{name}: release must resolve only crates.io {version}, never a path/git SDK')
    print('Registry SDKs verified: ' + ', '.join(f'{k}={v}' for k, v in versions.items()), file=sys.stderr)


def prepare(root, destination, versions, cargo, refresh=False):
    if destination.exists():
        raise ValueError(f'Refusing to overwrite an existing directory: {destination}')
    if run(['git', 'status', '--porcelain', '--untracked-files=no'], root, capture=True).strip():
        raise ValueError('Commit tracked source changes before preparing a release worktree')
    run(['git', 'worktree', 'add', '--detach', str(destination), 'HEAD'], root)
    try:
        prepare_manifests(destination, versions)
        if refresh:
            run([*cargo, 'update', '--workspace'], destination)
        else:
            if not (root / LOCK).is_file():
                raise ValueError('Missing registry-Cargo.lock; run --refresh-lock after publishing the SDKs')
            shutil.copy2(root / LOCK, destination / 'Cargo.lock')
        metadata = json.loads(run([*cargo, 'metadata', '--format-version', '1', '--locked'],
                                  destination, capture=True))
        verify_metadata(metadata, versions)
        if refresh:
            shutil.copy2(destination / 'Cargo.lock', root / LOCK)
    except BaseException:
        run(['git', 'worktree', 'remove', '--force', str(destination)], root)
        raise


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--prepare', type=Path, help='Create a persistent isolated CI source checkout')
    parser.add_argument('--refresh-lock', action='store_true', help='Refresh only the registry lock after an SDK release')
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args(argv)
    command = args.command
    if command and command[0] == '--':
        command = command[1:]
    if args.prepare and args.refresh_lock:
        parser.error('--prepare and --refresh-lock cannot be combined')
    if not command and not args.prepare and not args.refresh_lock:
        parser.error('Provide a Cargo command after --, or use --prepare/--refresh-lock')
    cargo = [os.environ.get('CARGO', 'cargo')]
    if command:
        cargo = command[:2] if len(command) > 1 and command[1].startswith('+') else command[:1]
    if (ROOT / 'dependencies.source.json').exists():
        raise ValueError('Resolve and remove dependencies.source.json before a registry release build')
    versions = load_versions(ROOT)
    if args.prepare:
        prepare(ROOT, args.prepare.resolve(), versions, cargo)
        return 0
    with tempfile.TemporaryDirectory(prefix='awiki-registry-build-') as temporary:
        checkout = Path(temporary) / 'source'
        prepare(ROOT, checkout, versions, cargo, args.refresh_lock)
        try:
            if command:
                env = os.environ.copy()
                target = Path(env.get('CARGO_TARGET_DIR', str(ROOT / 'target')))
                env['CARGO_TARGET_DIR'] = str(target if target.is_absolute() else ROOT / target)
                if '--locked' not in command:
                    command.append('--locked')
                run(command, checkout, env=env)
        finally:
            run(['git', 'worktree', 'remove', '--force', str(checkout)], ROOT)
    return 0


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        print(f'ERROR: {error}', file=sys.stderr)
        sys.exit(1)
