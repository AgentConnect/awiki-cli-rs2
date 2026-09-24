import json
from pathlib import Path
import unittest
import subprocess
import sys
from unittest.mock import patch

from acp_contract import ROOT, build_command, main, run_selection, test_binary, test_environment


class RunnerTests(unittest.TestCase):
    def test_source_mode_uses_locked_owner_builder_and_same_test_selection(self):
        command = build_command("cargo", "1.88.0", {"AWIKI_SOURCE_INTEGRATION": "1"})
        self.assertEqual(command[:7], [sys.executable, str(ROOT / "scripts/dependencies/build.py"),
                                      "--deps", "source", "--source-manifest", "dependencies.source.json",
                                      "--cargo-command"])
        self.assertEqual(command[7:], build_command("cargo", "1.88.0", {})[1:])
        self.assertIn("--locked", command)
        self.assertIn("--no-run", command)

    def test_registry_mode_keeps_owning_registry_gate(self):
        command = build_command("cargo", "1.88.0", {"AWIKI_RELEASE_REGISTRY": "1"})
        self.assertEqual(command[:3], [sys.executable, str(ROOT / "scripts/release/registry-build.py"), "--"])
        self.assertEqual(command[3:], build_command("cargo", "1.88.0", {}))

    def test_conflicting_dependency_modes_fail(self):
        with self.assertRaisesRegex(ValueError, "mutually exclusive"):
            build_command("cargo", "1.88.0", {
                "AWIKI_SOURCE_INTEGRATION": "1", "AWIKI_RELEASE_REGISTRY": "1"})

    def test_source_failure_does_not_fall_back_or_execute_tests(self):
        with patch.dict("os.environ", {
            "AWIKI_SOURCE_INTEGRATION": "1", "AWIKI_CLI_RUST_CARGO_TOOLCHAIN": "1.88.0",
        }, clear=True), patch.object(sys, "argv", ["acp_contract.py"]), \
                patch("acp_contract.shutil.which", side_effect=lambda tool: "/tools/" + tool), \
                patch("acp_contract.subprocess.check_output", return_value="24.12.0\n"), \
                patch("acp_contract.subprocess.run", return_value=subprocess.CompletedProcess([], 7)) as build, \
                patch("acp_contract.run_selection") as execute:
            self.assertEqual(main(), 7)
            build.assert_called_once()
            command = build.call_args.args[0]
            self.assertIn(str(ROOT / "scripts/dependencies/build.py"), command)
            self.assertIn("+1.88.0", command)
            execute.assert_not_called()

    def test_runtime_does_not_inherit_credentials_or_client_search_paths(self):
        with patch.dict("os.environ", {
            "NEW_PROVIDER_API_KEY": "private",
            "GEMINI_CLI_HOME": "/personal",
            "NODE_OPTIONS": "--require /personal/hook.js",
            "HTTPS_PROXY": "https://personal",
            "PATH": "/installed/agents",
        }):
            result = test_environment(Path("/temporary home"), Path("/tools"))
        self.assertNotIn("private", json.dumps(result))
        self.assertNotIn("personal", json.dumps(result))
        self.assertNotIn("/installed/agents", result["PATH"])
        self.assertEqual(result["HOME"], "/temporary home")
        self.assertEqual(result["XDG_CONFIG_HOME"], "/temporary home/config")
        self.assertEqual(result["RUST_TEST_THREADS"], "1")

    def test_build_requires_the_daemon_test_artifact(self):
        artifact = {"reason": "compiler-artifact", "target": {"name": "awiki_deamon"},
                    "profile": {"test": True}, "executable": "/build with spaces/test"}
        self.assertEqual(test_binary(json.dumps(artifact)), artifact["executable"])
        for changed in ({"reason": "build-finished"}, {"executable": None},
                        {"profile": {"test": False}}, {"target": {"name": "other"}}):
            with self.assertRaises(RuntimeError):
                test_binary(json.dumps({**artifact, **changed}))

    def test_registration_target_is_selected_explicitly(self):
        artifact = {"reason": "compiler-artifact", "target": {"name": "agent_registration_management"},
                    "profile": {"test": True}, "executable": "/tmp/registration-tests"}
        self.assertEqual(test_binary(json.dumps(artifact), "agent_registration_management"), artifact["executable"])
        with self.assertRaises(RuntimeError):
            test_binary(json.dumps(artifact))

    def test_empty_selection_cannot_pass(self):
        with patch("acp_contract.subprocess.check_output", return_value="0 tests, 0 benchmarks\n"), \
                patch("acp_contract.subprocess.Popen") as spawn:
            with self.assertRaisesRegex(RuntimeError, "No tests matched"):
                run_selection("unused", "renamed-module", {})
            spawn.assert_not_called()


if __name__ == "__main__":
    unittest.main()
