"""Registry release selection and source isolation contracts; no registry writes."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).with_name('registry-build.py')
spec = importlib.util.spec_from_file_location('registry_build', SCRIPT)
build = importlib.util.module_from_spec(spec)
spec.loader.exec_module(build)
VERSIONS = {'anp': '1.0.1', 'anp-identity': '0.2.1', 'awiki-im-core': '0.1.1'}


class RegistryBuildTests(unittest.TestCase):
    def test_rewrite_retains_feature_policy_and_removes_source_paths(self):
        text = 'im-core = { package = "awiki-im-core", path = "../im-core", default-features = false, features = ["http", "identity-external-provider"] }\n'
        result = build.registry_dependency(text, 'im-core', '0.1.1')
        self.assertNotIn('path =', result)
        self.assertIn('version = "=0.1.1"', result)
        self.assertIn('default-features = false', result)
        self.assertIn('features = ["http", "identity-external-provider"]', result)
        self.assertIn('package = "awiki-im-core"', result)

    def test_missing_or_duplicate_manifest_dependency_fails_closed(self):
        for source in ('[dependencies]\n', 'anp = { path = "a" }\nanp = { path = "b" }\n'):
            with self.assertRaisesRegex(ValueError, 'Expected one'):
                build.registry_dependency(source, 'anp', '1.0.1')

    def test_consumer_workspace_excludes_local_core(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / 'Cargo.toml').write_text('[workspace]\nmembers = [\n "crates/im-core",\n "crates/im-core-node",\n]\n[workspace.dependencies]\nanp = { path = "../anp", version = "1.0.0", default-features = false }\nanp-identity = { path = "../identity", features = ["key-import", "root-export"] }\n')
            for consumer in build.CONSUMERS:
                directory = root / 'crates' / consumer
                directory.mkdir(parents=True)
                (directory / 'Cargo.toml').write_text('im-core = { package = "awiki-im-core", path = "../im-core", features = ["blocking"] }\n')
            build.prepare_manifests(root, VERSIONS)
            manifest = (root / 'Cargo.toml').read_text()
            self.assertNotIn('"crates/im-core",', manifest)
            self.assertNotIn('path =', manifest)
            self.assertIn('"key-import", "root-export"', manifest)
            for consumer in build.CONSUMERS:
                self.assertIn('version = "=0.1.1"', (root / 'crates' / consumer / 'Cargo.toml').read_text())

    def test_metadata_rejects_local_stale_and_duplicate_sdk_sources(self):
        packages = [{'name': name, 'version': version, 'source': 'registry+https://github.com/rust-lang/crates.io-index'} for name, version in VERSIONS.items()]
        build.verify_metadata({'packages': packages}, VERSIONS)
        for source in [None, 'git+https://example.invalid/sdk']:
            changed = [dict(p) for p in packages]
            changed[0]['source'] = source
            with self.assertRaisesRegex(ValueError, 'never a path/git'):
                build.verify_metadata({'packages': changed}, VERSIONS)
        changed = [dict(p) for p in packages]
        changed[0]['version'] = '0.9.4'
        with self.assertRaises(ValueError):
            build.verify_metadata({'packages': changed}, VERSIONS)
        with self.assertRaises(ValueError):
            build.verify_metadata({'packages': packages + [{'name': 'anp', 'version': '1.0.1', 'source': None}]}, VERSIONS)

    def test_existing_destination_is_preserved(self):
        with tempfile.TemporaryDirectory() as temp, patch.object(build, 'run') as run:
            with self.assertRaisesRegex(ValueError, 'overwrite'):
                build.prepare(Path(temp), Path(temp), VERSIONS, ['cargo'])
            run.assert_not_called()

    def test_dirty_source_is_not_snapshotted(self):
        with tempfile.TemporaryDirectory() as temp, patch.object(build, 'run', return_value=' M Cargo.toml') as run:
            with self.assertRaisesRegex(ValueError, 'Commit tracked'):
                build.prepare(Path(temp), Path(temp) / 'new', VERSIONS, ['cargo'])
            self.assertEqual(run.call_count, 1)

    def test_release_entrypoints_use_registry_mode(self):
        root = SCRIPT.parents[2]
        for name in ['build-release-artifact.sh', 'daemon/_build-artifact.sh']:
            self.assertIn('registry-build.py', (root / 'scripts/release' / name).read_text())
        for name in ['build-linux.sh', 'build-apple.sh', 'build-android.sh', 'build-windows.ps1']:
            source = (root / 'scripts/flutter' / name).read_text()
            self.assertIn('AWIKI_RELEASE_REGISTRY', source)
            self.assertIn('registry-build.py', source)
        workflow = (root / '.github/workflows/im-core-node-artifacts.yml').read_text()
        self.assertIn('--prepare ../awiki-cli-registry', workflow)
        self.assertIn('working-directory: awiki-cli-registry', workflow)


if __name__ == '__main__':
    unittest.main()
