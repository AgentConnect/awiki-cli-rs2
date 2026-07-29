"""Focused notification-surface and host-event compatibility contracts.

[INPUT]: Representative host events, including redacted Device Join wakes
[OUTPUT]: Deterministic validation and Hermes surface projection assertions
[POS]: Unit contract for scripts/hermes_notify_adapter.py
"""

import importlib.util
import json
import pathlib
import sys
import unittest


MODULE_PATH = pathlib.Path(__file__).with_name("hermes_notify_adapter.py")
MODULE_SPEC = importlib.util.spec_from_file_location("hermes_notify_adapter", MODULE_PATH)
if MODULE_SPEC is None or MODULE_SPEC.loader is None:
    raise RuntimeError(f"failed to load module spec from {MODULE_PATH}")
hermes_notify_adapter = importlib.util.module_from_spec(MODULE_SPEC)
sys.modules[MODULE_SPEC.name] = hermes_notify_adapter
MODULE_SPEC.loader.exec_module(hermes_notify_adapter)


class HermesNotifyAdapterValidationTests(unittest.TestCase):
    def test_validate_notification_surface_accepts_valid_payload(self) -> None:
        payload = {
            "version": "1.0",
            "id": "ntf_msg_probe_001",
            "kind": "message",
            "topic": "im.message.received",
            "time": "2026-04-12T10:30:00Z",
            "binding_key": "awiki:direct:did:wba:test:bob:conv-probe-001",
            "source": {
                "network": "awiki",
                "account_id": "did:wba:test:bob",
                "conversation_id": "conv-probe-001",
                "thread_id": "msg-probe-001",
            },
            "data": {
                "sender_did": "did:wba:test:alice",
                "recipient_did": "did:wba:test:bob",
            },
        }

        validated = hermes_notify_adapter.validate_notification_surface(payload)

        self.assertEqual(validated["id"], "ntf_msg_probe_001")
        self.assertEqual(validated["topic"], "im.message.received")

    def test_validate_notification_surface_rejects_unexpected_top_level_field(self) -> None:
        payload = {
            "version": "1.0",
            "id": "ntf_msg_probe_001",
            "kind": "message",
            "topic": "im.message.received",
            "time": "2026-04-12T10:30:00Z",
            "binding_key": "awiki:direct:test",
            "source": {
                "network": "awiki",
                "account_id": "did:wba:test:bob",
                "conversation_id": "conv-probe-001",
                "thread_id": "msg-probe-001",
            },
            "data": {},
            "extra": "not-allowed",
        }

        with self.assertRaisesRegex(ValueError, "unexpected fields: extra"):
            hermes_notify_adapter.validate_notification_surface(payload)

    def test_validate_notification_surface_rejects_invalid_topic_and_time(self) -> None:
        payload = {
            "version": "1.0",
            "id": "ntf_msg_probe_001",
            "kind": "message",
            "topic": "IM_MESSAGE_RECEIVED",
            "time": "2026/04/12 10:30:00",
            "binding_key": "awiki:direct:test",
            "source": {
                "network": "awiki",
                "account_id": "did:wba:test:bob",
                "conversation_id": "conv-probe-001",
                "thread_id": "msg-probe-001",
            },
            "data": {},
        }

        with self.assertRaisesRegex(ValueError, "topic has invalid format"):
            hermes_notify_adapter.validate_notification_surface(payload)

        payload["topic"] = "im.message.received"
        with self.assertRaisesRegex(ValueError, "time must be RFC3339 date-time"):
            hermes_notify_adapter.validate_notification_surface(payload)

    def test_convert_host_event_to_surface_normalizes_and_validates(self) -> None:
        payload = {
            "version": "1.0",
            "id": "msg/probe/001",
            "topic": "im.message.received",
            "received_at": "2026-04-12T10:30:00Z",
            "data": {
                "message_id": "msg-probe-001",
                "conversation_id": "conv-probe-001",
                "sender_did": "did:wba:test:alice",
                "recipient_did": "did:wba:test:bob",
                "content_type": "text/plain",
                "text": "hello",
            },
        }

        surface = hermes_notify_adapter.convert_host_event_to_surface(payload)

        self.assertEqual(surface["id"], "ntf_msg_probe_001")
        self.assertEqual(surface["kind"], "message")
        self.assertEqual(surface["source"]["conversation_id"], "conv-probe-001")

    def test_convert_device_join_wake_uses_identity_scoped_binding(self) -> None:
        payload = {
            "version": "1.0",
            "id": "evt-join-001",
            "topic": "im.device.join.requested",
            "received_at": "2026-07-23T02:00:01Z",
            "data": {
                "channel": "device",
                "event_id": "evt-join-001",
                "join_session_id": "join-001",
                "recipient_did": "did:wba:test:bob",
                "issued_at": "2026-07-23T02:00:00Z",
                "expires_at": "2026-07-23T02:10:00Z",
            },
        }

        surface = hermes_notify_adapter.convert_host_event_to_surface(payload)

        self.assertEqual(surface["kind"], "event")
        self.assertEqual(
            surface["source"]["conversation_id"],
            "device-join:did:wba:test:bob",
        )
        self.assertEqual(surface["source"]["thread_id"], "join-001")
        self.assertEqual(
            surface["binding_key"],
            "awiki:device-join:did:wba:test:bob",
        )
        self.assertNotIn("sas", json.dumps(surface).lower())

    def test_convert_host_event_to_surface_rejects_invalid_received_at(self) -> None:
        payload = {
            "version": "1.0",
            "id": "msg-probe-001",
            "topic": "im.message.received",
            "received_at": "not-a-time",
            "data": {
                "message_id": "msg-probe-001",
                "sender_did": "did:wba:test:alice",
                "recipient_did": "did:wba:test:bob",
            },
        }

        with self.assertRaisesRegex(ValueError, "host event received_at must be RFC3339 date-time"):
            hermes_notify_adapter.convert_host_event_to_surface(payload)

    def test_convert_host_event_to_surface_rewrites_mail_topic_into_im_message(self) -> None:
        payload = {
            "version": "1.0",
            "id": "mail-msg-001",
            "topic": "mail.message.received",
            "received_at": "2026-04-12T10:30:00Z",
            "data": {
                "message_id": "mail-msg-001",
                "mailbox_address": "alice@example.com",
                "mailbox_did": "did:wba:test:alice",
                "recipient_did": "did:wba:test:alice",
                "from_addr": "sender@example.com",
                "subject": "Mail Subject",
            },
        }

        surface = hermes_notify_adapter.convert_host_event_to_surface(payload)

        self.assertEqual(surface["kind"], "message")
        self.assertEqual(surface["topic"], "im.message.received")
        self.assertEqual(surface["source"]["conversation_id"], "mail:alice@example.com")
        self.assertEqual(surface["source"]["thread_id"], "mail-msg-001")
        self.assertEqual(surface["binding_key"], "awiki:direct:did:wba:test:alice:mail:alice@example.com")
        self.assertEqual(surface["data"]["sender_handle"], "sender@example.com")
        self.assertEqual(surface["data"]["sender_did"], "mail:sender@example.com")
        self.assertEqual(surface["data"]["recipient_handle"], "alice@example.com")
        self.assertEqual(surface["data"]["content_type"], "text/plain")
        self.assertIn("[邮件]", surface["data"]["text"])
        self.assertIn("收件邮箱: alice@example.com", surface["data"]["text"])
        self.assertIn("发件人: sender@example.com", surface["data"]["text"])
        self.assertIn("主题: Mail Subject", surface["data"]["text"])

    def test_adapt_host_event_for_hermes_keeps_original_mail_fields(self) -> None:
        payload = {
            "version": "1.0",
            "id": "mail-msg-002",
            "topic": "mail.message.received",
            "received_at": "2026-04-12T10:31:00Z",
            "data": {
                "message_id": "mail-msg-002",
                "mailbox_address": "alice@example.com",
                "mailbox_did": "did:wba:test:alice",
                "recipient_did": "did:wba:test:alice",
                "from_addr": "sender@example.com",
                "subject": "Mail Subject 2",
                "preview": "Body preview",
                "has_attachments": True,
            },
        }

        adapted = hermes_notify_adapter.adapt_host_event_for_hermes(payload)

        self.assertEqual(adapted["topic"], "im.message.received")
        self.assertEqual(adapted["data"]["mailbox_address"], "alice@example.com")
        self.assertEqual(adapted["data"]["from_addr"], "sender@example.com")
        self.assertEqual(adapted["data"]["subject"], "Mail Subject 2")
        self.assertEqual(adapted["data"]["preview"], "Body preview")
        self.assertTrue(adapted["data"]["has_attachments"])
        self.assertIn("(这封邮件包含附件)", adapted["data"]["text"])


if __name__ == "__main__":
    unittest.main()
