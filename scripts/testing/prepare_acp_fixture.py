"""Offline component fixture for cross-service ACP tests; not a real adapter acceptance."""
from pathlib import Path
import json
import platform
import sys
import argparse


def prepare_acp_clients(repo: Path, root: Path) -> dict[str, str]:
    root = root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    specification = json.loads((repo / 'scripts/release/daemon/acp/components.json').read_text())
    source = (repo / 'crates/awiki-deamon/tests/fixtures/acp_agent.py').read_text()
    # Absolute interpreter avoids accidental dependence on a user's Python wrapper.
    source = source.replace('#!/usr/bin/env python3', '#!' + sys.executable, 1)
    binary_dir = root / '.local/bin'
    binary_dir.mkdir(parents=True)
    for name in ['hermes', 'codex', 'claude', 'opencode', 'gemini', 'kimi', 'dsh']:
        target = binary_dir / name
        target.write_text(source)
        target.chmod(0o700)
    components = root / 'test-components'
    components.mkdir()
    host = ('darwin' if sys.platform == 'darwin' else 'linux') + '-' + ('arm64' if platform.machine() in ('aarch64', 'arm64') else 'amd64')
    manifest = {'schema_version': 1, 'platform': host, 'available': True,
                'runtime': specification['runtime'], 'adapters': specification['adapters']}
    (components / 'manifest.json').write_text(json.dumps(manifest))
    for entry in specification['adapters'].values():
        path = components / entry['entry']
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text('offline fixture entry')
    node = binary_dir / 'node'
    node.write_text(source.replace("print('1.0.0')", "print('v24.0.0')"))
    node.chmod(0o700)
    return {'HOME': str(root), 'AWIKI_ACP_TEST_COMPONENTS_DIR': str(components), 'AWIKI_DAEMON_AGENT_PROXY_MODE': 'inherit'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True)
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[2]
    print(json.dumps(prepare_acp_clients(repo, args.root)))


if __name__ == '__main__':
    main()
