"""Offline fixture ownership and isolation contracts."""
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

from prepare_acp_fixture import prepare_acp_clients

ROOT = Path(__file__).resolve().parents[2]


class AcpFixtureTests(unittest.TestCase):
    def test_fresh_home_fixed_manifest_and_protocol(self):
        with tempfile.TemporaryDirectory() as temporary:
            home = Path(temporary) / 'clients'
            env = prepare_acp_clients(ROOT, home)
            spec = json.loads((ROOT / 'scripts/release/daemon/acp/components.json').read_text())
            components = Path(env['AWIKI_ACP_TEST_COMPONENTS_DIR'])
            self.assertEqual(json.loads((components / 'manifest.json').read_text())['adapters'], spec['adapters'])
            for executable in [home / '.local/bin/codex', home / '.local/bin/claude', components / 'node']:
                response = subprocess.run([str(executable)], input=json.dumps({'jsonrpc': '2.0', 'id': 7, 'method': 'initialize', 'params': {}})+'\n', capture_output=True, text=True, env=env, timeout=5, check=True)
                self.assertEqual(json.loads(response.stdout)['id'], 7)
                self.assertTrue(json.loads(response.stdout)['result']['agentCapabilities']['loadSession'])
            subprocess.run([str(home / '.local/bin/hermes'), 'acp', '--check'], env=env, timeout=5, check=True)
            self.assertFalse((home / '.hermes').exists())
            with self.assertRaises(FileExistsError):
                prepare_acp_clients(ROOT, home)


if __name__ == '__main__':
    unittest.main()
