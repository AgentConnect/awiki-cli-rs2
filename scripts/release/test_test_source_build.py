import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location('test_source_build', Path(__file__).with_name('test-source-build.py'))
build = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(build)


class TestSourceBuildTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.policy = json.loads(Path(__file__).with_name('singapore-test-sources.json').read_text())

    def read(self, policy):
        path = self.root / 'sources.json'
        path.write_text(json.dumps(policy))
        return build.read_manifest(path)

    def test_explicit_test_channel_and_full_sources(self):
        self.assertEqual(self.read(self.policy), self.policy)
        for change in ('channel', 'missing', 'branch', 'credential', 'escape'):
            value = copy.deepcopy(self.policy)
            if change == 'channel': value['channel'] = 'stable'
            if change == 'missing': del value['dependencies']['anp']
            if change == 'branch': value['dependencies']['anp']['commit'] = 'release/0910'
            if change == 'credential': value['dependencies']['anp']['repository'] = 'https://secret@github.com/agent-network-protocol/anp.git'
            if change == 'escape': value['dependencies']['anp']['checkout'] = '../external'
            with self.subTest(change=change), self.assertRaises(ValueError): self.read(value)

    def test_registry_duplicate_wrong_version_and_wrong_path_fail_closed(self):
        packages = []
        for name, relative in [('anp', 'anp/anp/rust'), ('anp-identity', 'anp/anp-identity/crates/anp-identity'), ('awiki-im-core', 'awiki-cli-rs2/crates/im-core')]:
            path = self.root / relative / 'Cargo.toml'
            path.parent.mkdir(parents=True)
            path.write_text('[package]\nversion = "1.2.3"\n')
            packages.append({'name': name, 'version': '1.2.3', 'source': None, 'manifest_path': str(path)})
        self.assertEqual(len(build.verify_metadata({'packages': packages}, self.root)), 3)
        for change in ('registry', 'duplicate', 'version', 'path', 'missing'):
            wrong = copy.deepcopy(packages)
            if change == 'registry': wrong[0]['source'] = 'registry+https://github.com/rust-lang/crates.io-index'
            if change == 'duplicate': wrong.append(wrong[0].copy())
            if change == 'version': wrong[0]['version'] = '1.2.2'
            if change == 'path': wrong[0]['manifest_path'] = str(self.root / 'old/Cargo.toml')
            if change == 'missing': wrong.pop()
            with self.subTest(change=change), self.assertRaises(ValueError):
                build.verify_metadata({'packages': wrong}, self.root)

    def test_publish_and_implicit_modes_are_rejected_before_build(self):
        manifest = str(Path(__file__).with_name('singapore-test-sources.json'))
        for args in [[], ['--', 'cargo', 'publish'], ['--', 'cargo', 'install', 'some-package'], ['--refresh-lock', '--', 'cargo', 'build']]:
            with self.subTest(args=args), self.assertRaises(SystemExit):
                build.main(['--manifest', manifest, *args])


if __name__ == '__main__':
    unittest.main()
