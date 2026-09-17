import unittest
from unittest.mock import patch
from compatibility import google_messages, install_compatibility


def call(identifier, name="shell"):
    return {"functionCall": {"id": identifier, "name": name, "args": {}}}


def result(identifier, name="shell", value="value"):
    return {"functionResponse": {"id": identifier, "name": name, "response": {"output": value}}}


class ConversionTests(unittest.TestCase):
    def test_parallel_and_serial_calls_preserve_exact_result_ids(self):
        converted = google_messages([
            {"role": "model", "parts": [call("a"), call("b")]},
            {"role": "user", "parts": [result("b"), result("a")]},
            {"role": "model", "parts": [call("c")]},
            {"role": "user", "parts": [result("c")]},
            {"role": "model", "parts": [{"text": "done"}]},
        ])
        self.assertEqual(converted[1]["tool_call_id"], converted[0]["tool_calls"][1]["id"])
        self.assertEqual(converted[2]["tool_call_id"], converted[0]["tool_calls"][0]["id"])
        self.assertEqual(converted[4]["tool_call_id"], converted[3]["tool_calls"][0]["id"])
        self.assertEqual(converted[-1]["content"], "done")

    def test_wrong_id_is_never_paired_by_unique_name(self):
        for invalid in [result("wrong"), result("a", "wrong"), {"functionResponse": {"name": "shell", "response": {}}}]:
            with self.assertRaises(ValueError):
                google_messages([{"role": "model", "parts": [call("a")]}, {"role": "user", "parts": [invalid]}])

    def test_system_image_and_all_results_preserved(self):
        converted = google_messages([
            {"role": "user", "parts": [{"text": "look"}, {"inlineData": {"mimeType": "image/png", "data": "exact"}}]},
            {"role": "model", "parts": [call("a")]},
            {"role": "user", "parts": [result("a", value="first"), result("a", value="second")]},
        ], {"parts": [{"text": "system"}]})
        self.assertEqual(converted[0]["content"], "system")
        self.assertEqual(converted[1]["content"][1]["image_url"]["url"], "data:image/png;base64,exact")
        self.assertIn('first', converted[-1]["content"])
        self.assertIn('second', converted[-1]["content"])

    def test_adapter_format_errors_are_safe_bad_requests(self):
        from litellm.exceptions import BadRequestError
        from litellm.google_genai.adapters.transformation import GoogleGenAIAdapter
        install_compatibility()
        install_compatibility()  # safe to initialize twice
        adapter = GoogleGenAIAdapter()
        for content in [
            [{"role": "user", "parts": [result("unknown")]}],
            [{"role": "model", "parts": [call("a")]}],
            [{"role": "model", "parts": [{"functionCall": {"name": "shell"}}]}],
            [{"role": "user", "parts": [{"inlineData": "PRIVATE_PAYLOAD"}]}],
        ]:
            with self.assertRaises(BadRequestError) as caught:
                adapter.translate_generate_content_to_completion("relay", content)
            self.assertEqual(caught.exception.status_code, 400)
            self.assertNotIn('PRIVATE_PAYLOAD', str(caught.exception))
        # Unexpected/system errors keep their classification, not a fake 400.
        with patch('compatibility.google_messages', side_effect=TimeoutError('timeout')):
            with self.assertRaises(TimeoutError):
                adapter.translate_generate_content_to_completion("relay", [])


if __name__ == '__main__':
    unittest.main()
