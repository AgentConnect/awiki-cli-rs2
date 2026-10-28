import http.client
import unittest
from unittest.mock import patch

import gate


class FixtureContract(unittest.TestCase):
    def test_macos_preferences_wait_for_live_exceptions(self):
        desired = {"HTTPEnable": 1, "HTTPProxy": "127.0.0.1", "HTTPPort": 8888,
                   "ExceptionsList": [gate.ADDRESS]}
        self.assertTrue(gate.MacProxy.settings_match(desired, 8888, gate.ADDRESS, ""))
        self.assertFalse(gate.MacProxy.settings_match({**desired, "ExceptionsList": []}, 8888, gate.ADDRESS, ""))
        self.assertFalse(gate.MacProxy.settings_match({**desired, "HTTPPort": 7777}, 8888, gate.ADDRESS, ""))

    def test_only_disposable_native_runners_can_mutate_settings(self):
        for environment, system in [({}, "Darwin"), ({"GITHUB_ACTIONS": "true"}, "Windows"),
                                    ({"GITHUB_ACTIONS": "true", "RUNNER_ENVIRONMENT": "self-hosted"}, "Windows"),
                                    ({"GITHUB_ACTIONS": "true", "RUNNER_ENVIRONMENT": "github-hosted"}, "Linux")]:
            with self.assertRaisesRegex(RuntimeError, "requires_disposable"):
                gate.require_disposable_runner(environment, system)
        for system in ("Darwin", "Windows"):
            gate.require_disposable_runner({"GITHUB_ACTIONS": "true", "RUNNER_ENVIRONMENT": "github-hosted"}, system)

    def test_windows_interface_mutation_targets_loopback_only(self):
        with patch.object(gate.platform, "system", return_value="Windows"), patch.object(gate, "run") as command:
            gate.alias(gate.ADDRESS, True)
            args = command.call_args.args
            self.assertIn("name=1", args)
            self.assertIn("skipassource=true", args)
            self.assertIn("store=active", args)

    def test_proxy_observes_headers_and_accepted_lost_response(self):
        server = gate.Fixture("proxy")
        try:
            client = http.client.HTTPConnection("127.0.0.1", server.port, timeout=2)
            client.request("CONNECT", "fixture.invalid:80")
            self.assertEqual(client.getresponse().status, 200)
            client.close()
            self.assertEqual(server.events, [{"method": "CONNECT", "business": False}])
            server.mode = "drop-write"
            client = http.client.HTTPConnection("127.0.0.1", server.port, timeout=2)
            client.request("POST", "/write", b"fixture", {"x-awiki-fixture": "business-only"})
            with self.assertRaises(http.client.RemoteDisconnected):
                client.getresponse()
            client.close()
            self.assertEqual(server.events[-1], {"method": "POST", "business": True})
        finally:
            server.close()

    def test_oracle_rejects_successful_wrong_path_and_auth_errors(self):
        for response in ({"ok": True, "status": 200, "body": "direct"},
                         {"ok": True, "status": 403, "body": "proxy"}, {"ok": False}):
            with self.assertRaises(AssertionError):
                gate.expect(response, "proxy")


if __name__ == "__main__":
    unittest.main()
