#!/usr/bin/env python3
"""固定源码的隔离测试包构建；不改变正式 registry 发布入口。"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('registry_build', ROOT / 'scripts/release/registry-build.py')
registry = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(registry)
LAYOUT = {'anp': ('anp/anp', 'rust'), 'anp-identity': ('anp/anp-identity', 'crates/anp-identity')}
LOCK = Path('scripts/release/test-source.Cargo.lock')


def read_manifest(path):
    data = json.loads(path.read_text())
    if (set(data) != {'schema_version', 'channel', 'dependencies'}
            or data['schema_version'] != 1 or data['channel'] != 'singapore-test'
            or set(data['dependencies']) != set(LAYOUT)):
        raise ValueError('Expected the explicit singapore-test source manifest')
    for name, item in data['dependencies'].items():
        if set(item) != {'repository', 'commit', 'checkout'}:
            raise ValueError(f'{name}: invalid source fields')
        expected = f'https://github.com/agent-network-protocol/{name}.git'
        if item['repository'] != expected or not re.fullmatch('[0-9a-f]{40}', item['commit']):
            raise ValueError(f'{name}: expected canonical repository and full immutable SHA')
        relative = Path(item['checkout'])
        if relative.is_absolute() or '..' in relative.parts or not relative.parts:
            raise ValueError(f'{name}: checkout must stay within the explicit sources root')
    return data


def verify_metadata(metadata, layout):
    expected = {'anp': layout / 'anp/anp/rust/Cargo.toml',
                'anp-identity': layout / 'anp/anp-identity/crates/anp-identity/Cargo.toml',
                'awiki-im-core': layout / 'awiki-cli-rs2/crates/im-core/Cargo.toml'}
    result = []
    for name, manifest in expected.items():
        packages = [p for p in metadata['packages'] if p['name'] == name]
        version = re.search(r'(?m)^version\s*=\s*"([^"]+)"', manifest.read_text()).group(1)
        if (len(packages) != 1 or packages[0].get('source') is not None
                or packages[0]['version'] != version
                or Path(packages[0]['manifest_path']).resolve() != manifest.resolve()):
            raise ValueError(f'{name}: exact archived source was not resolved')
        result.append({'name': name, 'version': version, 'source': 'unpublished-test-source'})
    return result


def prepare(layout, source_root, manifest_path, cargo, *, commit, refresh=False):
    if layout.exists():
        raise ValueError('Refusing to replace an existing build input directory')
    manifest = read_manifest(manifest_path)
    layout.mkdir(parents=True)
    checkout = layout / 'awiki-cli-rs2'
    registry.export_commit(ROOT, checkout, commit)
    # The source policy itself must come from the selected committed consumer.
    committed_manifest = checkout / manifest_path.resolve().relative_to(ROOT)
    if committed_manifest.read_bytes() != manifest_path.read_bytes():
        raise ValueError('Commit the source manifest before building')
    dependencies = {}
    for name, item in manifest['dependencies'].items():
        source = (source_root / item['checkout']).resolve()
        if not source.is_relative_to(source_root.resolve()):
            raise ValueError('Source checkout escapes the explicit sources root')
        destination = layout / LAYOUT[name][0]
        registry.export_commit(source, destination, item['commit'])
        dependencies[name] = {'repository': item['repository'], 'commit': item['commit'], 'published': False}
    lock = checkout / LOCK
    if not refresh:
        if not lock.is_file():
            raise ValueError('Missing committed test-source.Cargo.lock; explicitly refresh it first')
        shutil.copy2(lock, checkout / 'Cargo.lock')
    metadata_command = [*cargo, 'metadata', '--format-version', '1']
    if not refresh:
        metadata_command.append('--locked')
    metadata = json.loads(registry.run(metadata_command, checkout, capture=True))
    resolved = verify_metadata(metadata, layout)
    if refresh:
        shutil.copy2(checkout / 'Cargo.lock', ROOT / LOCK)
    receipt = {'schema_version': 1, 'dependency_mode': 'test-source', 'channel': 'singapore-test',
               'published': False, 'source_commit': commit, 'dependencies': dependencies,
               'resolved': resolved,
               'manifest_sha256': hashlib.sha256(manifest_path.read_bytes()).hexdigest(),
               'lock_sha256': hashlib.sha256((checkout / 'Cargo.lock').read_bytes()).hexdigest()}
    (layout / 'source-receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    return checkout, receipt


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--manifest', type=Path, required=True)
    parser.add_argument('--sources-root', type=Path, default=ROOT.parent)
    parser.add_argument('--source-commit')
    parser.add_argument('--prepare', type=Path)
    parser.add_argument('--provenance', type=Path)
    parser.add_argument('--refresh-lock', action='store_true')
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args(argv)
    command = args.command[1:] if args.command[:1] == ['--'] else args.command
    if args.refresh_lock and (command or args.prepare or args.provenance):
        parser.error('--refresh-lock cannot build or prepare persistent inputs')
    if not (args.refresh_lock or args.prepare or command):
        parser.error('Provide a Cargo command, --prepare or --refresh-lock')
    if command and (Path(command[0]).stem != 'cargo' or any(x in command for x in ('publish', 'install'))):
        parser.error('Only non-publishing Cargo commands are allowed')
    cargo = command[:2] if len(command) > 1 and command[1].startswith('+') else command[:1]
    cargo = cargo or ['cargo', '+1.88.0']
    commit = args.source_commit or registry.run(['git', 'rev-parse', 'HEAD'], ROOT, capture=True).strip()
    with tempfile.TemporaryDirectory(prefix='awiki-test-source-') as temporary:
        layout = args.prepare.resolve() if args.prepare else Path(temporary) / 'inputs'
        checkout, receipt = prepare(layout, args.sources_root.resolve(), args.manifest.resolve(), cargo,
                                    commit=commit, refresh=args.refresh_lock)
        if command:
            env = os.environ.copy()
            env['CARGO_TARGET_DIR'] = str(Path(env.get('CARGO_TARGET_DIR', str(ROOT / 'target'))).resolve())
            if '--locked' not in command:
                command.insert(command.index('--') if '--' in command else len(command), '--locked')
            registry.run(command, checkout, env=env)
            if hashlib.sha256((checkout / 'Cargo.lock').read_bytes()).hexdigest() != receipt['lock_sha256']:
                raise ValueError('Build changed the committed dependency lock')
            receipt['build_command'] = command
        if args.provenance:
            args.provenance.parent.mkdir(parents=True, exist_ok=True)
            args.provenance.write_text(json.dumps(receipt, indent=2) + '\n')
    return 0


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        print(f'ERROR: {error}', file=sys.stderr)
        sys.exit(1)
