"""Exercise the exact Python validator embedded in the public shell installer."""
import hashlib
import io
import json
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest

TEMPLATE = Path(__file__).with_name("_install.sh.template").read_text()
START = TEMPLATE.index("import sys\nimport tarfile\n")
VALIDATOR = TEMPLATE[START:TEMPLATE.index("\nPY\n}", START)]


def digest(value):
    return hashlib.sha256(value).hexdigest()


class InstallerComponentTests(unittest.TestCase):
    def archive(self, root, components=True, mutate=None, extra=None, link_runtime=False, host_node=False):
        files = {name: b"fixture" for name in ["awiki-deamon", "awiki-deamon-runtime", "README.txt", "LICENSE", "LICENSE-APACHE", "COMMERCIAL-LICENSING.md", "SOURCE.md"]}
        if components:
            payload = {"node": b"private node", "LICENSE.node": b"upstream notices", "package.json": b"{}", "package-lock.json": b"{}", "node_modules/codex/index.js": b"codex", "node_modules/claude/index.js": b"claude"}
            manifest = {"schema_version": 1, "available": True, "package_lock_sha256": digest(b"{}"),
                        "adapters": {"codex": {"entry": "node_modules/codex/index.js"}, "claude-code": {"entry": "node_modules/claude/index.js"}},
                        "files": {name: digest(value) for name, value in payload.items()}}
            if host_node:
                payload["node"] = b'#!/bin/sh\nexec /usr/bin/env node "$@"\n'
                payload["LICENSE.node"] = b'No Node binary is distributed.'
                manifest["runtime"] = {"kind": "host-node", "minimum_major": 22, "recommended_major": 24}
                manifest["files"] = {name: digest(value) for name, value in payload.items()}
            files.update({"acp/" + name: value for name, value in payload.items()})
            files["acp/manifest.json"] = json.dumps(manifest).encode()
        files["checksums.txt"] = "".join(f"{digest(data)}  {name}\n" for name, data in sorted(files.items())).encode()
        if mutate:
            mutate(files)
        archive = root / "package.tar.gz"
        with tarfile.open(archive, "w:gz") as tar:
            for name, data in files.items():
                entry = tarfile.TarInfo(name)
                if link_runtime and name == "awiki-deamon-runtime":
                    entry.type = tarfile.SYMTYPE
                    entry.linkname = "awiki-deamon"
                    tar.addfile(entry)
                else:
                    entry.size = len(data)
                    tar.addfile(entry, io.BytesIO(data))
            if extra:
                tar.addfile(extra)
        return archive

    def test_candidate_preflight_preserves_current_and_failed_first_install_can_retry(self):
        # Exercise the actual shell cutover, with isolated synthetic packages.
        start = TEMPLATE.index('INSTALL_DIR="${HOME}/.awiki-daemon/deamon/bin/${PKG_VERSION}"')
        end = TEMPLATE.index('set -- "$CURRENT_DIR/awiki-deamon" install --token', start)
        cutover = TEMPLATE[start:end].replace('${HOME}', '${FIXTURE_ROOT}')
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary_root = root / '.awiki-daemon/deamon/bin'
            old = binary_root / 'old'
            old.mkdir(parents=True)
            (old / 'awiki-deamon').write_text('old installation')
            current = binary_root / 'current'
            current.mkdir()
            (current / 'awiki-deamon').symlink_to('../old/awiki-deamon')
            original = (current / 'awiki-deamon').read_bytes()
            for good in (False, True):
                script = b'#!/bin/sh\n[ "$1" = __self-check ] || exit 7\nexit ' + (b'0' if good else b'9') + b'\n'
                archive = root / 'candidate.tar.gz'
                with tarfile.open(archive, 'w:gz') as tar:
                    entry = tarfile.TarInfo('awiki-deamon')
                    entry.size = len(script)
                    tar.addfile(entry, io.BytesIO(script))
                env = dict(__import__('os').environ, FIXTURE_ROOT=str(root), TMP_DIR=str(root / 'tmp'),
                           ARCHIVE_PATH=str(archive), PKG_VERSION='0.1.108-singapore.20261012')
                prefix = 'set -eu\ndie() { exit 1; }\nvalidate_extracted_package() { :; }\ncleanup_daemon_bin_root() { :; }\n'
                run = subprocess.run(['sh', '-c', prefix + cutover], env=env, capture_output=True)
                self.assertEqual(run.returncode == 0, good, run.stderr)
                if not good:
                    self.assertEqual((current / 'awiki-deamon').read_bytes(), original)
                    self.assertFalse((binary_root / env['PKG_VERSION']).exists())
                else:
                    self.assertEqual((current / 'awiki-deamon').read_bytes(), script)

    def test_host_node_package_preserves_legacy_installer_validation(self):
        self.assertEqual(self.validate(host_node=True)[0], 0)

    def validate(self, **kwargs):
        with tempfile.TemporaryDirectory() as temporary:
            archive = self.archive(Path(temporary), **kwargs)
            result = subprocess.run([sys.executable, "-", str(archive)], input=VALIDATOR, text=True, capture_output=True)
            return result.returncode, result.stderr

    def test_complete_components_and_flat_legacy_packages_are_accepted(self):
        for components in [True, False]:
            for link_runtime in [True, False]:
                with self.subTest(components=components, link_runtime=link_runtime):
                    self.assertEqual(self.validate(components=components, link_runtime=link_runtime), (0, ""))

    def test_rejects_tampered_node_and_unlisted_files(self):
        for mutate in [lambda files: files.update({"acp/node": b"changed"}),
                       lambda files: files.update({"acp/node_modules/injected.js": b"new"})]:
            code, detail = self.validate(mutate=mutate)
            self.assertNotEqual(code, 0)
            self.assertIn("checksums", detail)

    def test_rejects_component_links_and_path_traversal(self):
        for name, kind, target in [("acp/escape", tarfile.SYMTYPE, "../../outside"),
                                   ("acp/hard", tarfile.LNKTYPE, "awiki-deamon"),
                                   ("acp/../outside", tarfile.REGTYPE, ""),
                                   ("acp/./node", tarfile.REGTYPE, ""),
                                   ("acp/new\nline", tarfile.REGTYPE, "")]:
            with self.subTest(name=name):
                entry = tarfile.TarInfo(name)
                entry.type, entry.linkname = kind, target
                self.assertNotEqual(self.validate(extra=entry)[0], 0)

    def test_rejects_duplicate_entries_and_checksum_entries(self):
        self.assertNotEqual(self.validate(extra=tarfile.TarInfo("acp/node"))[0], 0)
        def duplicate(files):
            files["checksums.txt"] += files["checksums.txt"].splitlines(keepends=True)[0]
        self.assertNotEqual(self.validate(mutate=duplicate)[0], 0)

    def test_component_manifest_covers_every_dependency(self):
        def missing_manifest_file(files):
            manifest = json.loads(files["acp/manifest.json"])
            del manifest["files"]["node_modules/claude/index.js"]
            files["acp/manifest.json"] = json.dumps(manifest).encode()
            files["checksums.txt"] = "".join(f"{digest(data)}  {name}\n" for name, data in sorted(files.items()) if name != "checksums.txt").encode()
        code, detail = self.validate(mutate=missing_manifest_file)
        self.assertNotEqual(code, 0)
        self.assertIn("manifest checksums", detail)


if __name__ == "__main__":
    unittest.main()
