#!/usr/bin/env python3
"""Build pinned-registry consumers from a committed archive, without a worktree."""
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
import tarfile
import hashlib

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


def export_commit(root, destination, commit):
    if not re.fullmatch(r'[0-9a-f]{40}', commit):
        raise ValueError('source commit must be a full immutable Git SHA')
    actual = run(['git', 'rev-parse', commit + '^{commit}'], root, capture=True).strip()
    if actual != commit:
        raise ValueError('source commit did not resolve exactly')
    destination.mkdir(parents=True)
    with tempfile.TemporaryFile() as archive:
        subprocess.run(['git', 'archive', '--format=tar', commit], cwd=root, stdout=archive, check=True)
        archive.seek(0)
        with tarfile.open(fileobj=archive, mode='r:') as tree:
            for member in tree:
                relative = Path(member.name)
                if relative.is_absolute() or '..' in relative.parts or '.git' in relative.parts:
                    raise ValueError('source archive contains an unsafe path')
                if not (member.isdir() or member.isfile()):
                    raise ValueError('release archive must contain only regular source files')
                target = destination / relative
                if member.isdir():
                    target.mkdir(parents=True, exist_ok=True)
                else:
                    target.parent.mkdir(parents=True, exist_ok=True)
                    with target.open('xb') as output, tree.extractfile(member) as input:
                        shutil.copyfileobj(input, output)
                    target.chmod(0o755 if member.mode & 0o111 else 0o644)
    (destination / '.awiki-source.json').write_text(json.dumps({'commit': commit, 'source': 'git-archive', 'working_tree_edits_included': False}) + '\n')


def prepare(root, destination, versions, cargo, refresh=False, source_commit=None, metadata_output=None):
    if destination.exists():
        raise ValueError(f'Refusing to overwrite an existing directory: {destination}')
    if source_commit is None and run(['git', 'status', '--porcelain', '--untracked-files=no'], root, capture=True).strip():
        raise ValueError('Commit tracked source changes, or explicitly select --source-commit')
    commit = source_commit or run(['git', 'rev-parse', 'HEAD'], root, capture=True).strip()
    try:
        export_commit(root, destination, commit)
        if (destination / 'dependencies.source.json').exists():
            raise ValueError('selected commit still contains a source dependency manifest')
        if load_versions(destination) != versions:
            raise ValueError('selected commit and registry dependency policy differ')
        prepare_manifests(destination, versions)
        if refresh:
            run([*cargo, 'update', '--workspace'], destination)
        else:
            if not (destination / LOCK).is_file():
                raise ValueError('Missing registry-Cargo.lock; run --refresh-lock after publishing the SDKs')
            shutil.copy2(destination / LOCK, destination / 'Cargo.lock')
        metadata = json.loads(run([*cargo, 'metadata', '--format-version', '1', '--locked'],
                                  destination, capture=True))
        verify_metadata(metadata, versions)
        if metadata_output is not None:
            receipt={'source_commit':commit,'source':'git-archive','working_tree_edits_included':False,
                     'registry_lock_sha256':hashlib.sha256((destination/'Cargo.lock').read_bytes()).hexdigest(),
                     'sdk_packages':[{k:p[k] for k in ('name','version','source')} for p in metadata['packages'] if p['name'] in versions]}
            with metadata_output.open('x') as out:
                os.chmod(metadata_output,0o600)
                json.dump(receipt,out,indent=2);out.write('\n')
        if refresh:
            shutil.copy2(destination / 'Cargo.lock', root / LOCK)
    except BaseException:
        shutil.rmtree(destination, ignore_errors=True)
        raise
    return commit


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--prepare', type=Path, help='Export persistent committed CI build input without Git metadata')
    parser.add_argument('--source-commit', help='Build this full committed SHA while preserving unrelated working-tree edits')
    parser.add_argument('--metadata-output', type=Path, help='Write the verified source and registry dependency receipt to a new file')
    parser.add_argument('--refresh-lock', action='store_true', help='Refresh only the registry lock after an SDK release')
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args(argv)
    command = args.command
    if command and command[0] == '--':
        command = command[1:]
    if args.prepare and args.refresh_lock:
        parser.error('--prepare and --refresh-lock cannot be combined')
    if args.source_commit and args.refresh_lock:
        parser.error('--source-commit cannot refresh the working-tree registry lock')
    if not command and not args.prepare and not args.refresh_lock:
        parser.error('Provide a Cargo command after --, or use --prepare/--refresh-lock')
    cargo = [os.environ.get('CARGO', 'cargo')]
    if command:
        cargo = command[:2] if len(command) > 1 and command[1].startswith('+') else command[:1]
    if (ROOT / 'dependencies.source.json').exists():
        raise ValueError('Resolve and remove dependencies.source.json before a registry release build')
    versions = load_versions(ROOT)
    if args.prepare:
        prepare(ROOT, args.prepare.resolve(), versions, cargo, source_commit=args.source_commit,metadata_output=args.metadata_output.resolve() if args.metadata_output else None)
        return 0
    with tempfile.TemporaryDirectory(prefix='awiki-registry-build-') as temporary:
        checkout = Path(temporary) / 'source'
        commit = prepare(ROOT, checkout, versions, cargo, args.refresh_lock, args.source_commit,args.metadata_output.resolve() if args.metadata_output else None)
        try:
            if command:
                env = os.environ.copy()
                if env.get('AWIKI_CLI_COMMIT', commit) != commit:
                    raise ValueError('build metadata commit differs from the selected source')
                env['AWIKI_CLI_COMMIT'] = commit
                target = Path(env.get('CARGO_TARGET_DIR', str(ROOT / 'target')))
                env['CARGO_TARGET_DIR'] = str(target if target.is_absolute() else ROOT / target)
                if '--locked' not in command:
                    command.append('--locked')
                run(command, checkout, env=env)
        finally:
            shutil.rmtree(checkout)
    return 0


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        print(f'ERROR: {error}', file=sys.stderr)
        sys.exit(1)
