import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location('label_package', Path(__file__).with_name('label-test-package.py'))
labeler = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(labeler)


class LabelTestPackageTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        manifest = {'name': '@awiki/im-core-node', 'version': '0.2.6', 'private': True,
                    'optionalDependencies': {'@awiki/im-core-node-darwin-arm64': '0.2.6'}}
        provenance = {'localCandidate': True, 'package': {'name': manifest['name'], 'version': manifest['version']},
                      'binarySha256': 'unchanged', 'source': {'commit': 'a' * 40}}
        sbom = {'metadata': {'component': provenance['package'].copy()},
                'components': [{'name': '@awiki/im-core-node-darwin-arm64', 'version': '0.2.6'}]}
        for name, value in [('package.json', manifest), ('provenance.json', provenance), ('sbom.cdx.json', sbom)]:
            (self.root / name).write_text(json.dumps(value))
        (self.root / 'SOURCE.md').write_text('Source revision\n')
        (self.root / 'addon.node').write_bytes(b'fixed native input')
        entries = [{'path': p.name, 'sha256': hashlib.sha256(p.read_bytes()).hexdigest()} for p in self.root.iterdir()]
        (self.root / 'checksums.json').write_text(json.dumps({'files': entries}))

    def test_relabels_metadata_and_pins_without_changing_native_input(self):
        labeler.label(self.root, '0.2.8-sg.20260922.1')
        manifest = json.loads((self.root / 'package.json').read_text())
        self.assertEqual(set(manifest['optionalDependencies'].values()), {manifest['version']})
        provenance = json.loads((self.root / 'provenance.json').read_text())
        self.assertFalse(provenance['published'])
        self.assertEqual(provenance['source']['commit'], 'a' * 40)
        self.assertEqual((self.root / 'addon.node').read_bytes(), b'fixed native input')
        for entry in json.loads((self.root / 'checksums.json').read_text())['files']:
            self.assertEqual(hashlib.sha256((self.root / entry['path']).read_bytes()).hexdigest(), entry['sha256'])

    def test_rejects_registry_versions_and_tampered_native_input(self):
        with self.assertRaises(ValueError): labeler.label(self.root, '0.2.8')
        (self.root / 'addon.node').write_bytes(b'wrong library')
        with self.assertRaisesRegex(ValueError, 'changed'): labeler.label(self.root, '0.2.8-sg.20260922.1')

    def test_rejects_packages_without_candidate_provenance(self):
        (self.root / 'provenance.json').write_text('{}')
        with self.assertRaises(ValueError): labeler.label(self.root, '0.2.8-sg.20260922.1')
