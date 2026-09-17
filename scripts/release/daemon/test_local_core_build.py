"""临时源码包必须包含准确 Core，其他 SDK 及默认正式流程保持原边界。"""
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).with_name('local-core-build.py')
spec = importlib.util.spec_from_file_location('local_core_build', SCRIPT)
build = importlib.util.module_from_spec(spec)
spec.loader.exec_module(build)
VERSIONS = {'anp': '1.0.3', 'anp-identity': '0.2.3', 'awiki-im-core': '0.1.5'}


class LocalCoreBuildTests(unittest.TestCase):
    def test_only_core_keeps_its_workspace_source(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            manifest = root / 'Cargo.toml'
            manifest.write_text('[workspace]\nmembers = ["crates/im-core"]\n[workspace.dependencies]\nanp = { path = "../anp", default-features = false }\nanp-identity = { path = "../identity", features = ["key-import"] }\n')
            build.prepare_manifests(root, VERSIONS)
            text = manifest.read_text()
            self.assertIn('"crates/im-core"', text)
            self.assertNotIn('path =', text)
            self.assertIn('version = "=1.0.3"', text)
            self.assertIn('version = "=0.2.3"', text)
            self.assertIn('default-features = false', text)
            self.assertIn('features = ["key-import"]', text)

    def test_metadata_rejects_fallback_wrong_core_path_and_other_local_sdks(self):
        root = Path('/isolated')
        packages = [
            {'name': name, 'version': version,
             'source': None if name == 'awiki-im-core' else 'registry+https://github.com/rust-lang/crates.io-index',
             'manifest_path': str(root / 'crates/im-core/Cargo.toml')}
            for name, version in VERSIONS.items()
        ]
        build.verify_metadata({'packages': packages}, VERSIONS, root)
        for index, key, value in [
            (2, 'source', 'registry+https://github.com/rust-lang/crates.io-index'),
            (2, 'manifest_path', '/other/crates/im-core/Cargo.toml'),
            (0, 'source', None),
            (1, 'source', 'git+https://example.invalid/identity'),
            (2, 'version', '0.1.4'),
        ]:
            changed = [dict(p) for p in packages]
            changed[index][key] = value
            with self.subTest(index=index, key=key, value=value), self.assertRaises(ValueError):
                build.verify_metadata({'packages': changed}, VERSIONS, root)
        with self.assertRaises(ValueError):
            build.verify_metadata({'packages': packages + [packages[2]]}, VERSIONS, root)

    def test_dirty_source_is_rejected_before_creating_worktree(self):
        with patch.object(build.registry, 'run', return_value=' M Cargo.toml') as run:
            with self.assertRaisesRegex(ValueError, 'Commit tracked'):
                build.prepare(Path('/source'), Path('/temporary'), VERSIONS, ['cargo'])
            self.assertEqual(run.call_count, 1)

    def test_metadata_failure_removes_only_owned_worktree(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            checkout = root / 'checkout'
            checkout.mkdir()
            (root / build.LOCK).parent.mkdir(parents=True)
            (root / build.LOCK).write_text('lock')
            with patch.object(build.registry, 'run', side_effect=['', '', '{"packages": []}', '']) as run, patch.object(build, 'prepare_manifests'):
                with self.assertRaises(ValueError):
                    build.prepare(root, checkout, VERSIONS, ['cargo'])
                self.assertEqual(run.call_args.args[0], ['git', 'worktree', 'remove', '--force', str(checkout)])

    def test_artifact_default_is_registry_and_local_requires_explicit_option(self):
        command = ['bash', str(SCRIPT.with_name('_build-artifact.sh')), '--dry-run']
        default = subprocess.check_output(command, text=True)
        local = subprocess.check_output([*command, '--local-core'], text=True)
        self.assertIn('registry-build.py', default)
        self.assertNotIn('local-core-build.py', default)
        self.assertIn('local-core-build.py', local)
        self.assertNotIn('registry-build.py', local)


if __name__ == '__main__':
    unittest.main()
