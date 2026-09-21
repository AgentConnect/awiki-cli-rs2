import importlib.util
import json
from pathlib import Path
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

    def test_prunes_auxiliary_files_preserving_runtime_and_licenses(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            keep = ["sdk/index.js", "sdk/config.json", "sdk/src/index.ts", "sdk/LICENSE.md", "sdk/tests/NOTICE.txt", "sdk/package.json"]
            remove = ["sdk/index.js.map", "sdk/index.d.ts", "sdk/index.d.mts", "sdk/test/spec.js", "sdk/dist/acp.test.js", "sdk/README.md", "sdk/examples/demo.js"]
            for name in keep + remove:
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("fixture")
            self.assertGreater(builder.prune_development_assets(root), 0)
            self.assertTrue(all((root / name).is_file() for name in keep))
            self.assertTrue(all(not (root / name).exists() for name in remove))

    def test_upgrade_compatibility_launcher_is_not_a_node_binary(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            builder.write_legacy_upgrade_launcher(root)
            self.assertTrue((root / "node").read_text().startswith("#!/bin/sh"))
            self.assertLess((root / "node").stat().st_size, 1024)
            self.assertIn("No Node.js binary", (root / "LICENSE.node").read_text())

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
