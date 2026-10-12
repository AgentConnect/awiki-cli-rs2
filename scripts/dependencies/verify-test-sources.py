#!/usr/bin/env python3
"""Fail closed before direct Android test builds; no registry SDK fallback."""
import json
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[2]
manifest = json.loads((root / 'dependencies.source.json').read_text())
metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--locked', '--format-version', '1'], cwd=root, text=True))
resolved = {}
for name in ('anp', 'anp-identity', 'awiki-im-core'):
    matches = [p for p in metadata['packages'] if p['name'] == name]
    assert len(matches) == 1 and matches[0]['source'] is None, f'{name}: expected one source SDK'
    directory = Path(matches[0]['manifest_path']).parent
    sha = subprocess.check_output(['git', '-C', str(directory), 'rev-parse', 'HEAD'], text=True).strip()
    expected = subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True).strip() if name == 'awiki-im-core' else manifest['dependencies'][name]['commit']
    assert sha == expected, f'{name}: source SHA mismatch'
    assert not subprocess.check_output(['git', '-C', str(directory), 'diff', '--name-only', 'HEAD', '--', '.'], text=True).strip(), f'{name}: modified SDK source'
    resolved[name] = {'commit': sha, 'source': 'local', 'version': matches[0]['version']}
output = root / '.artifacts/dependencies/android-test'
output.mkdir(parents=True, exist_ok=True)
(output / 'resolution.json').write_text(json.dumps(resolved, indent=2) + '\n')
print(json.dumps(resolved))
