#!/usr/bin/env python3
"""为已审计的本地候选包分配独立测试版本，随后仍须运行原有 pack audit。"""
import argparse
import hashlib
import json
from pathlib import Path
import re
from urllib.parse import quote


def label(directory, version):
    if not re.fullmatch(r'\d+\.\d+\.\d+-sg\.20260922\.\d+', version):
        raise ValueError('Expected an explicitly namespaced Singapore test version')
    manifest = json.loads((directory / 'package.json').read_text())
    provenance = json.loads((directory / 'provenance.json').read_text())
    if (manifest.get('private') is not True or provenance.get('localCandidate') is not True
            or provenance.get('package') != {'name': manifest['name'], 'version': manifest['version']}):
        raise ValueError('Only matching, private, local candidate packages may be relabeled')
    for item in json.loads((directory / 'checksums.json').read_text())['files']:
        relative = Path(item['path'])
        if relative.is_absolute() or '..' in relative.parts or (directory / relative).is_symlink():
            raise ValueError('Unsafe candidate checksum path')
        if hashlib.sha256((directory / relative).read_bytes()).hexdigest() != item['sha256']:
            raise ValueError('Candidate contents changed before relabeling')
    provenance['upstreamPackageVersion'] = manifest['version']
    provenance['package']['version'] = version
    provenance['channel'] = 'singapore-test'
    provenance['published'] = False
    manifest['version'] = version
    for name in manifest.get('optionalDependencies', {}):
        if not name.startswith(manifest['name'] + '-'):
            raise ValueError('Unexpected optional dependency in native wrapper')
        manifest['optionalDependencies'][name] = version
    sbom = json.loads((directory / 'sbom.cdx.json').read_text())
    sbom['metadata']['component']['version'] = version
    for item in sbom['components']:
        if item['name'] in manifest.get('optionalDependencies', {}):
            item['version'] = version
            item['purl'] = 'pkg:npm/' + quote(item['name'], safe='') + '@' + version
    for name, value in [('package.json', manifest), ('provenance.json', provenance), ('sbom.cdx.json', sbom)]:
        (directory / name).write_text(json.dumps(value, indent=2) + '\n')
    with (directory / 'SOURCE.md').open('a') as output:
        output.write(f'\nSingapore test package: {manifest["name"]}@{version}\nSDK publication: unpublished\n')
    files = []
    for path in sorted(directory.rglob('*')):
        if path.is_symlink():
            raise ValueError('Candidate must not contain symlinks')
        if path.is_file() and path.name != 'checksums.json':
            files.append({'path': path.relative_to(directory).as_posix(),
                          'sha256': hashlib.sha256(path.read_bytes()).hexdigest()})
    (directory / 'checksums.json').write_text(json.dumps({'schemaVersion': 1, 'files': files}, indent=2) + '\n')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--directory', type=Path, required=True)
    parser.add_argument('--version', required=True)
    args = parser.parse_args()
    label(args.directory, args.version)
