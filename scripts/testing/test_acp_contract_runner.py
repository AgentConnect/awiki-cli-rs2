import json
from pathlib import Path
import unittest
from unittest.mock import patch

from acp_contract import run_selection, test_binary, test_environment


class RunnerTests(unittest.TestCase):
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

    def test_build_requires_the_daemon_test_artifact(self):
        artifact = {"reason": "compiler-artifact", "target": {"name": "awiki_deamon"},
                    "profile": {"test": True}, "executable": "/build with spaces/test"}
        self.assertEqual(test_binary(json.dumps(artifact)), artifact["executable"])
        for changed in ({"reason": "build-finished"}, {"executable": None},
                        {"profile": {"test": False}}, {"target": {"name": "other"}}):
            with self.assertRaises(RuntimeError):
                test_binary(json.dumps({**artifact, **changed}))

    def test_empty_selection_cannot_pass(self):
        with patch("acp_contract.subprocess.check_output", return_value="0 tests, 0 benchmarks\n"), \
                patch("acp_contract.subprocess.Popen") as spawn:
            with self.assertRaisesRegex(RuntimeError, "No tests matched"):
                run_selection("unused", "renamed-module", {})
            spawn.assert_not_called()


if __name__ == "__main__":
    unittest.main()
