import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("acp_components", Path(__file__).with_name("prepare-acp-components.py"))
builder = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(builder)


class AcpComponentTests(unittest.TestCase):
    def test_checked_in_lock_contains_only_fixed_npm_artifacts(self):
        lock = json.loads((builder.SOURCE / "package-lock.json").read_text())
        builder.validate_lock(lock)
        config = json.loads((builder.SOURCE / "components.json").read_text())
        for adapter in config["adapters"].values():
            self.assertEqual(lock["packages"]["node_modules/" + adapter["package"]]["version"], adapter["version"])

    def test_lock_rejects_git_local_links_and_unchecked_downloads(self):
        for package in [
            {"resolved": "file:../../private"},
            {"resolved": "git+https://github.com/example/adapter"},
            {"resolved": "https://registry.npmjs.org/a.tgz"},
            {"resolved": "https://user:password@registry.npmjs.org/a.tgz", "integrity": "sha512-value"},
            {"resolved": "https://elsewhere.test/a.tgz", "integrity": "sha512-value"},
            {"resolved": "https://registry.npmjs.org/a.tgz", "integrity": "sha512-value", "link": True},
        ]:
            with self.subTest(package=package), self.assertRaises(ValueError):
                builder.validate_lock({"lockfileVersion": 3, "packages": {"node_modules/a": package}})

    def test_extracts_only_runtime_and_notices(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / "node-test.tar.gz"
            with tarfile.open(archive, "w:gz") as tar:
                for name, data in [("node-test/bin/node", b"node"), ("node-test/LICENSE", b"notice"), ("../outside", b"bad")]:
                    member = tarfile.TarInfo(name)
                    member.size = len(data)
                    tar.addfile(member, io.BytesIO(data))
            output = root / "out"
            output.mkdir()
            builder.extract_node(archive, output)
            self.assertEqual((output / "node").read_bytes(), b"node")
            self.assertEqual((output / "LICENSE.node").read_bytes(), b"notice")
            self.assertEqual((output / "node").stat().st_mode & 0o777, 0o755)
            self.assertFalse((root / "outside").exists())

    def test_rejects_symlinked_node_and_missing_notices(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / "node-test.tar.gz"
            with tarfile.open(archive, "w:gz") as tar:
                member = tarfile.TarInfo("node-test/bin/node")
                member.type = tarfile.SYMTYPE
                member.linkname = "../../outside"
                tar.addfile(member)
            with self.assertRaises(ValueError):
                builder.extract_node(archive, root)

    def test_inventory_rejects_symlinks_and_checksum_line_injection(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            path = root / "unsafe"
            path.symlink_to("/etc/passwd")
            with self.assertRaises(ValueError):
                builder.component_files(root)
            path.unlink()
            (root / "line\ninjection").write_text("bad")
            with self.assertRaises(ValueError):
                builder.component_files(root)

    def test_unknown_platform_has_explicit_unavailable_manifest_without_npm(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output = root / "acp"
            builder.prepare("linux-arm64", output, root / "cache")
            manifest = json.loads((output / "manifest.json").read_text())
            self.assertFalse(manifest["available"])
            self.assertEqual(manifest["unavailable_reason"], "adapter_platform_unsupported")
            self.assertFalse((output / "node").exists())


if __name__ == "__main__":
    unittest.main()
