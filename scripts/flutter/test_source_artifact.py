"""Source Apple provenance rejects changed dependencies and native bytes."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('artifact', Path(__file__).with_name('native-artifact-manifest.py'))
artifact = importlib.util.module_from_spec(spec)
spec.loader.exec_module(artifact)

class SourceArtifactTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.target = 'aarch64-apple-darwin'
        self.sha = 'a' * 40
        self.selection = {'commit': 'b' * 40, 'repository': 'https://github.com/AgentConnect/awiki-cli-rs2.git'}
        manifest = self.root / 'dependencies.source.json'
        manifest.write_text(json.dumps({'dependencies': {'awiki-im-core': self.selection}}))
        lock = self.root / 'dependencies.source.Cargo.lock'
        lock.write_text('locked-dependencies')
        self.base = self.root / '.artifacts/dependencies/source'
        self.archive = self.base / 'target' / self.target / 'release/libawiki_im_core.a'
        self.archive.parent.mkdir(parents=True)
        self.archive.write_bytes(b'native-library')
        self.evidence = {
            'mode': 'source', 'consumer': {'commit': self.sha, 'dirty': False},
            'source_manifest_sha256': artifact.sha256_file(manifest),
            'source_lock_sha256': artifact.sha256_file(lock),
            'dependencies': {'awiki-im-core': dict(self.selection, dirty=False)},
            'build': {'target': self.target, 'optimized': True, 'no_default_features': True},
            'archive_sha256': artifact.sha256_file(self.archive),
        }
        self.save()

    def save(self):
        (self.base / (self.target + '.json')).write_text(json.dumps(self.evidence))

    def verify(self):
        with patch.object(artifact, 'ROOT', self.root), patch.object(artifact, 'run', return_value=self.sha.encode()):
            return artifact.source_integration_record([self.target])

    def test_exact_build_evidence_is_retained(self):
        self.assertEqual(self.verify()[self.target], self.evidence)

    def test_replaced_archive_is_rejected(self):
        self.archive.write_bytes(b'old-or-unrelated-library')
        with self.assertRaisesRegex(artifact.ManifestError, 'archive changed'):
            self.verify()

    def test_changed_lock_is_rejected(self):
        (self.root / 'dependencies.source.Cargo.lock').write_text('different-dependency')
        with self.assertRaisesRegex(artifact.ManifestError, 'input provenance'):
            self.verify()

    def test_wrong_dependency_commit_is_rejected(self):
        self.evidence['dependencies']['awiki-im-core']['commit'] = 'c' * 40
        self.save()
        with self.assertRaisesRegex(artifact.ManifestError, 'dependency provenance'):
            self.verify()

    def test_dirty_or_different_consumer_is_rejected(self):
        for consumer in [{'commit': self.sha, 'dirty': True}, {'commit': 'd' * 40, 'dirty': False}]:
            with self.subTest(consumer=consumer):
                self.evidence['consumer'] = consumer
                self.save()
                with self.assertRaisesRegex(artifact.ManifestError, 'input provenance'):
                    self.verify()

    def test_wrong_native_target_is_rejected(self):
        self.evidence['build']['target'] = 'aarch64-apple-ios'
        self.save()
        with self.assertRaisesRegex(artifact.ManifestError, 'build options'):
            self.verify()
