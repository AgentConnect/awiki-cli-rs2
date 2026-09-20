#!/usr/bin/env python3
"""隔离的 SDK 消费端构建：默认 registry，显式 local/source 联调。"""
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
from urllib.parse import urlsplit

ROOT = Path(__file__).resolve().parents[2]
SPECS = {
    'anp': ('anp/anp', 'rust'),
    'anp-identity': ('anp/anp-identity', 'crates/anp-identity'),
    'awiki-im-core': ('awiki-cli-rs2', 'crates/im-core'),
}
spec = importlib.util.spec_from_file_location('registry_build', ROOT / 'scripts/release/registry-build.py')
registry = importlib.util.module_from_spec(spec)
spec.loader.exec_module(registry)


def run(args, cwd=None, capture=False, env=None):
    return subprocess.run(args, cwd=cwd, env=env, check=True, text=True, encoding="utf-8",
                          stdout=subprocess.PIPE if capture else None).stdout


def read_selection(path, mode):
    data = json.loads(path.read_text(encoding="utf-8"))
    entries = data.get('dependencies')
    if data.get('schema_version') != 1 or not isinstance(entries, dict) or not entries:
        raise ValueError('Expected schema_version=1 and nonempty dependencies')
    for name, item in entries.items():
        if name not in SPECS or not isinstance(item, dict):
            raise ValueError(f'Unsupported SDK: {name}')
        if mode == 'local':
            if set(item) != {'path'} or not isinstance(item['path'], str) or not item['path']:
                raise ValueError('Local entries contain only a repository path')
        else:
            if set(item) != {'repository', 'commit', 'pull_request'}:
                raise ValueError('Source entries require repository, commit and pull_request')
            url = urlsplit(item['repository'])
            if url.scheme != 'https' or not url.hostname or url.username or url.password or url.query or url.fragment:
                raise ValueError('Source repository must be a credential-free HTTPS URL')
            if not re.fullmatch('[0-9a-f]{40}', item['commit']):
                raise ValueError('Source dependencies require an exact 40-character commit, not a branch')
            if not isinstance(item['pull_request'], str) or not item['pull_request'].startswith('https://'):
                raise ValueError('Source dependencies require a reviewable PR URL')
    return entries


def copy_source(source, destination):
    """Snapshot tracked + nonignored new source, including dirty development edits."""
    source = source.resolve()
    names = run(['git', 'ls-files', '-z', '--cached', '--others', '--exclude-standard'], source, True)
    destination.mkdir(parents=True)
    for name in set(names.split('\0')) - {''}:
        relative = Path(name)
        if relative.is_absolute() or '..' in relative.parts:
            raise ValueError('Invalid source path')
        original = source / relative
        if original.is_symlink():
            # A source snapshot must never pull ignored caches or files outside its repo.
            raise ValueError(f'Source symlink requires explicit packaging: {name}')
        if not original.is_file():
            continue
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(original, target)
    digest = hashlib.sha256()
    for file in sorted(p for p in destination.rglob('*') if p.is_file()):
        digest.update(file.relative_to(destination).as_posix().encode() + b'\0')
        digest.update(hashlib.sha256(file.read_bytes()).digest())
    return {'tree_sha256': digest.hexdigest(), 'commit': run(['git', 'rev-parse', 'HEAD'], source, True).strip(),
            'dirty': bool(run(['git', 'status', '--porcelain'], source, True).strip())}


def checkout_source(item, destination):
    destination.mkdir(parents=True)
    run(['git', 'init', '--quiet', str(destination)])
    run(['git', '-c', 'credential.helper=', 'fetch', '--depth=1', item['repository'], item['commit']], destination)
    actual = run(['git', 'rev-parse', 'FETCH_HEAD'], destination, True).strip()
    if actual != item['commit']:
        raise ValueError('Fetched source does not match the declared commit')
    run(['git', '-c', 'core.hooksPath=/dev/null', 'checkout', '--detach', '--quiet', actual], destination)
    return {'commit': actual, 'dirty': False}


def normalize_sdk_roots(roots, versions):
    # Local SDK repositories may themselves contain sibling paths. Rewrite only
    # their staged workspace declarations; unselected dependencies stay registry.
    for name in ('anp-identity', 'awiki-im-core'):
        if name not in roots:
            continue
        manifest = roots[name] / 'Cargo.toml'
        text = manifest.read_text(encoding="utf-8")
        for dependency in ('anp', 'anp-identity') if name == 'awiki-im-core' else ('anp',):
            text = registry.registry_dependency(text, dependency, versions[dependency])
        manifest.write_text(text, encoding="utf-8")


def apply_patches(checkout, roots):
    if not roots:
        return
    text = '\n[patch.crates-io]\n'
    for name, root in roots.items():
        directory = root / SPECS[name][1]
        text += f'{json.dumps(name)} = {{ path = {json.dumps(str(directory))} }}\n'
    with (checkout / 'Cargo.toml').open('a', encoding='utf-8') as stream:
        stream.write(text)


def verify_resolution(metadata, versions, roots):
    for name, version in versions.items():
        packages = [p for p in metadata['packages'] if p['name'] == name]
        if len(packages) != 1 or packages[0]['version'] != version:
            raise ValueError(f'{name}: missing, duplicate or incompatible SDK version')
        package = packages[0]
        if name in roots:
            expected = (roots[name] / SPECS[name][1] / 'Cargo.toml').resolve()
            if package.get('source') is not None or Path(package['manifest_path']).resolve() != expected:
                raise ValueError(f'{name}: local override was not used')
        elif package.get('source') != 'registry+https://github.com/rust-lang/crates.io-index':
            raise ValueError(f'{name}: unselected SDK must resolve from crates.io')


def source_cargo_command(arguments):
    """Accept only locked development build/check/test operations."""
    args = list(arguments)
    prefix = []
    if args and args[0].startswith('+'):
        if not re.fullmatch(r'\+[0-9]+\.[0-9]+(?:\.[0-9]+)?', args[0]):
            raise ValueError('Source Cargo toolchain must be an exact numeric version')
        prefix.append(args.pop(0))
    if not args or args[0] not in ('build', 'check', 'test'):
        raise ValueError('Source Cargo accepts only build, check or test')
    # These would escape the audited workspace, dependency graph or output root.
    forbidden = ('--manifest-path', '--lockfile-path', '--target-dir', '--config', '--profile')
    if any(arg == '--' or arg.split('=')[0] in forbidden or arg.startswith('-Z') for arg in args):
        raise ValueError('Source Cargo cannot override workspace, configuration or output ownership')
    return [os.environ.get('CARGO', 'cargo'), *prefix, *args, *([] if '--locked' in args else ['--locked'])]


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--profile', choices=['debug', 'release'], default='debug')
    parser.add_argument('--deps', choices=['registry', 'local', 'source'], default='registry')
    parser.add_argument('--local-config', type=Path)
    parser.add_argument('--source-manifest', type=Path)
    parser.add_argument('--package', default='awiki-cli', choices=['awiki-cli', 'awiki-deamon', 'im-core-dart', 'awiki-im-core-node'])
    parser.add_argument('--cargo-command', nargs=argparse.REMAINDER, help='Locked source-only build/check/test command, including optional numeric toolchain')
    parser.add_argument('--check', action='store_true')
    parser.add_argument('--resolve-only', action='store_true', help='只校验实际依赖图，不编译')
    parser.add_argument('--refresh-lock', action='store_true', help='仅刷新提交用的源码联调锁文件')
    parser.add_argument('--target', choices=['aarch64-apple-darwin', 'x86_64-apple-darwin', 'aarch64-apple-ios', 'aarch64-apple-ios-sim', 'x86_64-apple-ios'])
    parser.add_argument('--features')
    parser.add_argument('--no-default-features', action='store_true')
    parser.add_argument('--optimized', action='store_true', help='Optimize a Debug source integration build; not a release gate')
    args = parser.parse_args(argv)
    if any((args.target, args.features, args.no_default_features, args.optimized)) and (args.deps != 'source' or args.profile != 'debug' or args.package != 'im-core-dart' or args.refresh_lock or args.resolve_only):
        parser.error('Native build options require a Debug source im-core-dart build/check')
    if args.profile == 'release' and (ROOT / 'dependencies.source.json').exists():
        parser.error('Resolve and remove dependencies.source.json before release')
    if args.profile == 'release' and args.resolve_only:
        parser.error('Release must execute its build/check, not resolution only')
    if args.profile == 'release' and args.deps != 'registry':
        parser.error('Release accepts only registry dependencies')
    selection = args.local_config if args.deps == 'local' else args.source_manifest
    if args.deps == 'registry' and (args.local_config or args.source_manifest or args.refresh_lock):
        parser.error('Registry mode does not accept local/source overrides')
    if args.deps != 'registry' and (selection is None or bool(args.local_config) == bool(args.source_manifest)):
        parser.error('Select exactly one matching local config or source manifest')
    if args.refresh_lock and args.deps != 'source':
        parser.error('--refresh-lock is only for source dependencies')
    if args.cargo_command is not None and (args.deps != 'source' or args.profile != 'debug' or args.check or args.resolve_only or args.refresh_lock or args.target or args.features or args.no_default_features or args.optimized):
        parser.error('--cargo-command requires source development mode without fixed build options')
    if args.cargo_command is not None and os.environ.get('AWIKI_RELEASE_REGISTRY') == '1':
        parser.error('Source Cargo and registry release mode are mutually exclusive')
    command = source_cargo_command(args.cargo_command) if args.cargo_command is not None else [os.environ.get('CARGO', 'cargo'), 'check' if args.check else 'build', '-p', args.package]
    if args.target:
        command += ['--target', args.target]
    if args.features:
        command += ['--features', args.features]
    if args.no_default_features:
        command.append('--no-default-features')
    if args.optimized:
        command.append('--release')
    if args.profile == 'release':
        command.append('--release')
        # Existing release entrypoint checks clean committed source and registry metadata.
        return registry.main(['--', *command])
    versions = registry.load_versions(ROOT)
    entries = read_selection(selection.resolve(), args.deps) if selection else {}
    artifacts = ROOT / '.artifacts/dependencies' / args.deps
    artifacts.mkdir(parents=True, exist_ok=True)
    if args.cargo_command is not None:
        (artifacts / 'command-result.json').unlink(missing_ok=True)
    with tempfile.TemporaryDirectory(prefix='awiki-deps-') as temporary:
        layout = Path(temporary)
        checkout = layout / 'consumer'
        evidence = {'mode': args.deps, 'profile': args.profile, 'consumer': copy_source(ROOT, checkout), 'dependencies': {}}
        roots = {}
        for name, item in entries.items():
            destination = layout / 'sources' / SPECS[name][0]
            evidence['dependencies'][name] = (copy_source((selection.resolve().parent / item['path']).resolve(), destination)
                if args.deps == 'local' else checkout_source(item, destination))
            if args.deps == 'source':
                evidence['dependencies'][name]['repository'] = item['repository']
            roots[name] = destination
        # A development replacement may have its own next/prerelease version.
        # Apply that exact version only inside this isolated consumer snapshot.
        versions = dict(versions)
        for name, root in roots.items():
            text = (root / SPECS[name][1] / 'Cargo.toml').read_text(encoding="utf-8")
            match = re.search(r'(?m)^version\s*=\s*"([^"]+)"', text)
            if not match:
                raise ValueError(f'{name}: SDK must declare its package version')
            versions[name] = match[1]
        normalize_sdk_roots(roots, versions)
        registry.prepare_manifests(checkout, versions)
        apply_patches(checkout, roots)
        lock = ROOT / registry.LOCK
        if args.deps == 'source':
            lock = selection.resolve().with_suffix('.Cargo.lock')
            if not args.refresh_lock and not lock.is_file():
                raise ValueError('Source PR requires its committed .Cargo.lock; generate with --refresh-lock')
        if lock.is_file():
            shutil.copy2(lock, checkout / 'Cargo.lock')
        env = os.environ.copy()
        env['CARGO_TARGET_DIR'] = str(artifacts / 'target')
        metadata_cmd = [command[0], *([command[1]] if len(command) > 1 and command[1].startswith('+') else []), 'metadata', '--format-version', '1']
        if args.deps != 'local' and not args.refresh_lock:
            metadata_cmd.append('--locked')
        metadata = json.loads(run(metadata_cmd, checkout, True, env))
        verify_resolution(metadata, versions, roots)
        if args.refresh_lock:
            shutil.copy2(checkout / 'Cargo.lock', lock)
        evidence['resolved'] = [{'name': p['name'], 'version': p['version'], 'source': p['source']}
                                for p in metadata['packages'] if p['name'] in versions]
        (artifacts / 'resolution.json').write_text(json.dumps(evidence, indent=2) + '\n', encoding='utf-8')
        if not args.refresh_lock and not args.resolve_only:
            run(command if '--locked' in command else [*command, '--locked'], checkout, env=env)
            if args.cargo_command is not None:
                evidence['command'] = command
                evidence['source_manifest_sha256'] = hashlib.sha256(selection.read_bytes()).hexdigest()
                evidence['source_lock_sha256'] = hashlib.sha256(lock.read_bytes()).hexdigest()
                (artifacts / 'command-result.json').write_text(json.dumps(evidence, indent=2) + '\n', encoding='utf-8')
            if args.target and not args.check:
                evidence['source_manifest_sha256'] = hashlib.sha256(selection.read_bytes()).hexdigest()
                evidence['source_lock_sha256'] = hashlib.sha256(lock.read_bytes()).hexdigest()
                evidence['build'] = {'target': args.target, 'features': sorted((args.features or '').split(',')), 'optimized': args.optimized, 'no_default_features': args.no_default_features}
                archive = artifacts / 'target' / args.target / ('release' if args.optimized else 'debug') / 'libawiki_im_core.a'
                evidence['archive_sha256'] = hashlib.sha256(archive.read_bytes()).hexdigest()
                (artifacts / (args.target + '.json')).write_text(json.dumps(evidence, indent=2) + '\n', encoding='utf-8')
    return 0


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        print(f'ERROR: {error}', file=sys.stderr)
        sys.exit(1)
