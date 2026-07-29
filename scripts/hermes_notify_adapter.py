#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.10"
# ///
"""Hermes notify adapter for awiki Notification Surface v1.

[INPUT]: Signed Notification Surface v1 or compatible HostNotificationEvent
[OUTPUT]: Validated, identity-scoped Hermes webhook events
[POS]: Compatibility ingress and projection boundary for the Hermes host sink

Endpoints:
- GET  /healthz
- POST /notify
- POST /notify/host-event  (compat endpoint for awiki HostNotificationEvent)
"""

from __future__ import annotations

import argparse
import hmac
import json
import logging
import re
import threading
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from hashlib import sha256
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any
from urllib import error as urllib_error
from urllib import request as urllib_request

LOGGER = logging.getLogger("hermes_notify_adapter")

_NOTIFY_ID_RE = re.compile(r"[^A-Za-z0-9._:-]+")
_NOTIFY_SURFACE_ID_RE = re.compile(r"^ntf_[A-Za-z0-9._:-]+$")
_TOPIC_RE = re.compile(r"^[a-z0-9]+(\.[a-z0-9_]+)+$")
_NETWORK_RE = re.compile(r"^[a-z0-9][a-z0-9._-]*$")

_SURFACE_REQUIRED_FIELDS = {"version", "id", "kind", "topic", "time", "binding_key", "source", "data"}
_SURFACE_SOURCE_REQUIRED_FIELDS = {"network", "account_id", "conversation_id", "thread_id"}
_HOST_EVENT_REQUIRED_FIELDS = {"version", "id", "topic", "received_at", "data"}


@dataclass(slots=True)
class AdapterConfig:
    notify_secret: str
    notify_max_skew_seconds: int
    max_body_bytes: int
    dedupe_ttl_seconds: int
    dedupe_max_entries: int
    hermes_webhook_url: str
    hermes_route_secret: str
    hermes_timeout_seconds: float


class DedupeCache:
    """In-memory TTL dedupe cache keyed by notification id."""

    def __init__(self, ttl_seconds: int, max_entries: int) -> None:
        self._ttl_seconds = max(ttl_seconds, 1)
        self._max_entries = max(max_entries, 128)
        self._store: dict[str, float] = {}
        self._lock = threading.RLock()

    def is_duplicate(self, key: str) -> bool:
        now = time.time()
        with self._lock:
            self._cleanup_locked(now)
            expires = self._store.get(key, 0.0)
            return expires > now

    def remember(self, key: str) -> None:
        now = time.time()
        with self._lock:
            self._cleanup_locked(now)
            if len(self._store) >= self._max_entries:
                oldest_key = min(self._store.items(), key=lambda item: item[1])[0]
                self._store.pop(oldest_key, None)
            self._store[key] = now + self._ttl_seconds

    def _cleanup_locked(self, now: float) -> None:
        expired = [key for key, expires in self._store.items() if expires <= now]
        for key in expired:
            self._store.pop(key, None)


class HermesNotifyAdapterServer(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, address: tuple[str, int], config: AdapterConfig) -> None:
        super().__init__(address, HermesNotifyAdapterHandler)
        self.config = config
        self.dedupe = DedupeCache(
            ttl_seconds=config.dedupe_ttl_seconds,
            max_entries=config.dedupe_max_entries,
        )


class HermesNotifyAdapterHandler(BaseHTTPRequestHandler):
    server: HermesNotifyAdapterServer
    protocol_version = "HTTP/1.1"

    def log_message(self, format_string: str, *args: Any) -> None:
        LOGGER.info("%s - %s", self.address_string(), format_string % args)

    def do_GET(self) -> None:  # noqa: N802
        if self.path == "/healthz":
            self._write_json(
                HTTPStatus.OK,
                {
                    "ok": True,
                    "time": utc_now(),
                    "host": "hermes",
                },
            )
            return
        self._write_error(HTTPStatus.NOT_FOUND, "not_found", "path not found")

    def do_POST(self) -> None:  # noqa: N802
        if self.path == "/notify":
            self._handle_notify(expect_host_event=False)
            return
        if self.path == "/notify/host-event":
            self._handle_notify(expect_host_event=True)
            return
        self._write_error(HTTPStatus.NOT_FOUND, "not_found", "path not found")

    def _handle_notify(self, *, expect_host_event: bool) -> None:
        payload, raw_body = self._read_json_body()
        if payload is None or raw_body is None:
            return
        try:
            verify_notify_signature(
                raw_body=raw_body,
                timestamp_header=self.headers.get("X-Notify-Timestamp", ""),
                signature_header=self.headers.get("X-Notify-Signature", ""),
                secret=self.server.config.notify_secret,
                max_skew_seconds=self.server.config.notify_max_skew_seconds,
            )
            if expect_host_event:
                surface = convert_host_event_to_surface(payload)
            else:
                surface = validate_notification_surface(payload)
        except ValueError as exc:
            self._write_error(HTTPStatus.BAD_REQUEST, "invalid_request", str(exc))
            return
        except PermissionError as exc:
            self._write_error(HTTPStatus.UNAUTHORIZED, "unauthorized", str(exc))
            return

        notify_id = surface["id"]
        if self.server.dedupe.is_duplicate(notify_id):
            self._write_json(
                HTTPStatus.ACCEPTED,
                {
                    "accepted": True,
                    "id": notify_id,
                    "host": "hermes",
                    "ref": "duplicate",
                    "duplicate": True,
                },
            )
            return

        try:
            hermes_ref = forward_to_hermes(surface, self.server.config)
        except RuntimeError as exc:
            self._write_error(HTTPStatus.BAD_GATEWAY, "upstream_failed", str(exc))
            return

        self.server.dedupe.remember(notify_id)
        self._write_json(
            HTTPStatus.ACCEPTED,
            {
                "accepted": True,
                "id": notify_id,
                "host": "hermes",
                "ref": hermes_ref,
            },
        )

    def _read_json_body(self) -> tuple[dict[str, Any] | None, bytes | None]:
        content_type = (self.headers.get("Content-Type", "") or "").lower()
        if "application/json" not in content_type:
            self._write_error(
                HTTPStatus.BAD_REQUEST,
                "invalid_content_type",
                "Content-Type must be application/json",
            )
            return None, None

        content_length_raw = self.headers.get("Content-Length", "0") or "0"
        try:
            content_length = int(content_length_raw)
        except ValueError:
            self._write_error(HTTPStatus.BAD_REQUEST, "invalid_content_length", "invalid Content-Length")
            return None, None
        if content_length <= 0:
            self._write_error(HTTPStatus.BAD_REQUEST, "empty_body", "request body is required")
            return None, None
        if content_length > self.server.config.max_body_bytes:
            self._write_error(
                HTTPStatus.REQUEST_ENTITY_TOO_LARGE,
                "body_too_large",
                f"request body exceeds {self.server.config.max_body_bytes} bytes",
            )
            return None, None

        raw_body = self.rfile.read(content_length)
        try:
            payload = json.loads(raw_body.decode("utf-8"))
        except Exception as exc:  # pylint: disable=broad-except
            self._write_error(HTTPStatus.BAD_REQUEST, "invalid_json", f"invalid JSON: {exc}")
            return None, None
        if not isinstance(payload, dict):
            self._write_error(HTTPStatus.BAD_REQUEST, "invalid_json", "request body must be a JSON object")
            return None, None
        return payload, raw_body

    def _write_error(self, status: HTTPStatus, code: str, message: str, details: dict[str, Any] | None = None) -> None:
        payload = {
            "accepted": False,
            "error": {
                "code": code,
                "message": message,
            },
        }
        if details:
            payload["error"]["details"] = details
        self._write_json(status, payload)

    def _write_json(self, status: HTTPStatus, payload: dict[str, Any]) -> None:
        raw = json.dumps(payload, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)


def utc_now() -> str:
    return datetime.now(tz=timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def verify_notify_signature(
    *,
    raw_body: bytes,
    timestamp_header: str,
    signature_header: str,
    secret: str,
    max_skew_seconds: int,
) -> None:
    timestamp_text = timestamp_header.strip()
    if not timestamp_text:
        raise PermissionError("missing X-Notify-Timestamp")
    try:
        timestamp = int(timestamp_text)
    except ValueError as exc:
        raise PermissionError("invalid X-Notify-Timestamp") from exc
    now = int(time.time())
    if abs(now - timestamp) > max(max_skew_seconds, 1):
        raise PermissionError("request timestamp outside replay window")

    signature_text = signature_header.strip()
    if signature_text.startswith("sha256="):
        signature_text = signature_text[len("sha256=") :]
    if not signature_text:
        raise PermissionError("missing X-Notify-Signature")

    signing_input = timestamp_text.encode("utf-8") + b"." + raw_body
    expected = hmac.new(secret.encode("utf-8"), signing_input, sha256).hexdigest()
    if not hmac.compare_digest(signature_text.lower(), expected.lower()):
        raise PermissionError("signature verification failed")


def validate_notification_surface(payload: dict[str, Any]) -> dict[str, Any]:
    missing = sorted(_SURFACE_REQUIRED_FIELDS - set(payload.keys()))
    if missing:
        raise ValueError(f"missing required fields: {', '.join(missing)}")
    extra = sorted(set(payload.keys()) - _SURFACE_REQUIRED_FIELDS)
    if extra:
        raise ValueError(f"unexpected fields: {', '.join(extra)}")
    if str(payload.get("version")) != "1.0":
        raise ValueError("version must be 1.0")
    notify_id = require_text_field("id", payload.get("id"), min_length=8, max_length=160, pattern=_NOTIFY_SURFACE_ID_RE)
    kind = require_text_field("kind", payload.get("kind"))
    if kind not in {"message", "state", "event"}:
        raise ValueError("kind must be message, state, or event")
    topic = require_text_field("topic", payload.get("topic"), min_length=3, max_length=160, pattern=_TOPIC_RE)
    event_time = require_rfc3339_field("time", payload.get("time"))
    binding_key = require_text_field("binding_key", payload.get("binding_key"), min_length=3, max_length=512)

    source = payload.get("source")
    if not isinstance(source, dict):
        raise ValueError("source must be an object")
    missing_source = sorted(_SURFACE_SOURCE_REQUIRED_FIELDS - set(source.keys()))
    if missing_source:
        raise ValueError(f"missing required source fields: {', '.join(missing_source)}")
    extra_source = sorted(set(source.keys()) - _SURFACE_SOURCE_REQUIRED_FIELDS)
    if extra_source:
        raise ValueError(f"unexpected source fields: {', '.join(extra_source)}")

    data = payload.get("data")
    if not isinstance(data, dict):
        raise ValueError("data must be an object")

    return {
        "version": "1.0",
        "id": notify_id,
        "kind": kind,
        "topic": topic,
        "time": event_time,
        "binding_key": binding_key,
        "source": {
            "network": require_text_field("source.network", source.get("network"), min_length=2, max_length=64, pattern=_NETWORK_RE),
            "account_id": require_text_field("source.account_id", source.get("account_id"), min_length=1, max_length=256),
            "conversation_id": require_text_field(
                "source.conversation_id",
                source.get("conversation_id"),
                min_length=1,
                max_length=512,
            ),
            "thread_id": require_text_field("source.thread_id", source.get("thread_id"), min_length=1, max_length=512),
        },
        "data": data,
    }


def convert_host_event_to_surface(payload: dict[str, Any]) -> dict[str, Any]:
    payload = adapt_host_event_for_hermes(payload)
    missing = sorted(_HOST_EVENT_REQUIRED_FIELDS - set(payload.keys()))
    if missing:
        raise ValueError(f"host event missing fields: {', '.join(missing)}")
    if str(payload.get("version")) != "1.0":
        raise ValueError("host event version must be 1.0")

    data = payload.get("data")
    if not isinstance(data, dict):
        raise ValueError("host event data must be an object")

    event_id = require_text_field("host event id", payload.get("id"), min_length=1, max_length=512)
    topic = require_text_field("host event topic", payload.get("topic"), min_length=3, max_length=160, pattern=_TOPIC_RE)
    event_time = require_rfc3339_field("host event received_at", payload.get("received_at"))

    recipient_did = str(data.get("recipient_did", "")).strip() or "unknown"
    conversation_id = resolve_conversation_id(topic, data, event_id)
    thread_id = resolve_thread_id(topic, data, event_id)
    binding_key = resolve_binding_key(topic, recipient_did, conversation_id, data, event_id)

    surface = {
        "version": "1.0",
        "id": normalize_notify_id(event_id),
        "kind": resolve_kind(topic),
        "topic": topic,
        "time": event_time,
        "binding_key": binding_key,
        "source": {
            "network": "awiki",
            "account_id": recipient_did,
            "conversation_id": conversation_id,
            "thread_id": thread_id,
        },
        "data": data,
    }
    return validate_notification_surface(surface)


def adapt_host_event_for_hermes(payload: dict[str, Any]) -> dict[str, Any]:
    if str(payload.get("topic", "")).strip() != "mail.message.received":
        return payload

    data = payload.get("data")
    if not isinstance(data, dict):
        return payload

    message_id = str(data.get("message_id", "")).strip() or str(payload.get("id", "")).strip() or "unknown"
    mailbox_address = str(data.get("mailbox_address", "")).strip()
    mailbox_did = str(data.get("mailbox_did", "")).strip()
    recipient_did = str(data.get("recipient_did", "")).strip() or mailbox_did or "unknown"
    from_addr = str(data.get("from_addr", "")).strip()
    subject = str(data.get("subject", "")).strip()
    preview = str(data.get("preview", "")).strip()
    has_attachments = bool(data.get("has_attachments", False))

    synthetic_data = dict(data)
    synthetic_data.update(
        {
            "channel": "direct",
            "message_id": message_id,
            "conversation_id": resolve_mail_conversation_id(mailbox_address, recipient_did, message_id),
            "sender_handle": from_addr,
            "sender_did": resolve_mail_sender_id(from_addr),
            "recipient_handle": mailbox_address,
            "recipient_did": recipient_did,
            "content_type": "text/plain",
            "text": build_mail_im_text(mailbox_address, from_addr, subject, preview, has_attachments),
            "created_at": str(payload.get("received_at", "")).strip(),
        }
    )

    synthetic_event = dict(payload)
    synthetic_event["topic"] = "im.message.received"
    synthetic_event["data"] = synthetic_data
    return synthetic_event


def resolve_mail_conversation_id(mailbox_address: str, recipient_did: str, fallback: str) -> str:
    base = mailbox_address or recipient_did or fallback or "unknown"
    return f"mail:{base}"


def resolve_mail_sender_id(from_addr: str) -> str:
    sender = from_addr or "unknown"
    return f"mail:{sender}"


def build_mail_im_text(
    mailbox_address: str,
    from_addr: str,
    subject: str,
    preview: str,
    has_attachments: bool,
) -> str:
    lines = ["[邮件]"]
    if mailbox_address:
        lines.append(f"收件邮箱: {mailbox_address}")
    if from_addr:
        lines.append(f"发件人: {from_addr}")
    if subject:
        lines.append(f"主题: {subject}")
    if preview:
        lines.extend(["", preview])
    if has_attachments:
        lines.extend(["", "(这封邮件包含附件)"])
    return "\n".join(lines)


def normalize_notify_id(host_event_id: str) -> str:
    event_id = host_event_id.strip()
    if not event_id:
        event_id = f"evt_{int(time.time() * 1000)}"
    if event_id.startswith("ntf_"):
        return event_id
    safe = _NOTIFY_ID_RE.sub("_", event_id).strip("._:-")
    if not safe:
        safe = f"evt_{int(time.time() * 1000)}"
    return f"ntf_{safe}"


def require_text_field(
    field_name: str,
    value: Any,
    *,
    min_length: int = 1,
    max_length: int | None = None,
    pattern: re.Pattern[str] | None = None,
) -> str:
    text = str(value or "").strip()
    if not text:
        raise ValueError(f"{field_name} is required")
    if len(text) < min_length:
        raise ValueError(f"{field_name} must be at least {min_length} characters")
    if max_length is not None and len(text) > max_length:
        raise ValueError(f"{field_name} must be at most {max_length} characters")
    if pattern is not None and not pattern.fullmatch(text):
        raise ValueError(f"{field_name} has invalid format")
    return text


def require_rfc3339_field(field_name: str, value: Any) -> str:
    text = require_text_field(field_name, value)
    normalized = text.replace("Z", "+00:00")
    try:
        parsed = datetime.fromisoformat(normalized)
    except ValueError as exc:
        raise ValueError(f"{field_name} must be RFC3339 date-time") from exc
    if parsed.tzinfo is None:
        raise ValueError(f"{field_name} must include timezone information")
    return text


def resolve_kind(topic: str) -> str:
    if topic in {"im.message.received", "im.group.message.received", "mail.message.received"}:
        return "message"
    if topic == "im.group.state.changed":
        return "state"
    return "event"


def resolve_conversation_id(topic: str, data: dict[str, Any], fallback: str) -> str:
    if topic == "im.device.join.requested":
        recipient_did = str(data.get("recipient_did", "")).strip() or "unknown"
        return f"device-join:{recipient_did}"
    if topic == "im.message.received":
        return str(data.get("conversation_id", "")).strip() or fallback or "unknown"
    if topic == "mail.message.received":
        return str(data.get("mailbox_address", "")).strip() or str(data.get("recipient_did", "")).strip() or fallback or "unknown"
    if topic in {"im.group.message.received", "im.group.state.changed"}:
        return str(data.get("group_did", "")).strip() or fallback or "unknown"
    return fallback or "unknown"


def resolve_thread_id(topic: str, data: dict[str, Any], fallback: str) -> str:
    if topic == "im.device.join.requested":
        return str(data.get("join_session_id", "")).strip() or fallback or "unknown"
    if topic in {"im.message.received", "im.group.message.received", "mail.message.received"}:
        return str(data.get("message_id", "")).strip() or fallback or "unknown"
    if topic == "im.group.state.changed":
        return str(data.get("event_id", "")).strip() or fallback or "unknown"
    return fallback or "unknown"


def resolve_binding_key(
    topic: str,
    recipient_did: str,
    conversation_id: str,
    data: dict[str, Any],
    fallback: str,
) -> str:
    if topic == "im.device.join.requested":
        return f"awiki:device-join:{recipient_did}"
    if topic == "im.message.received":
        sender_did = str(data.get("sender_did", "")).strip()
        conversation_part = conversation_id if conversation_id and conversation_id != "unknown" else sender_did
        return f"awiki:direct:{recipient_did}:{conversation_part or fallback or 'unknown'}"
    if topic == "mail.message.received":
        mailbox = str(data.get("mailbox_address", "")).strip() or conversation_id or fallback or "unknown"
        return f"awiki:mail:{recipient_did}:{mailbox}"
    if topic == "im.group.message.received":
        group_did = str(data.get("group_did", "")).strip() or fallback or "unknown"
        return f"awiki:group:{recipient_did}:{group_did}"
    if topic == "im.group.state.changed":
        group_did = str(data.get("group_did", "")).strip() or fallback or "unknown"
        return f"awiki:group-state:{recipient_did}:{group_did}"
    return f"awiki:fallback:{topic or 'unknown'}:{fallback or 'unknown'}"


def forward_to_hermes(surface: dict[str, Any], config: AdapterConfig) -> str:
    notify_payload = json.dumps(surface, ensure_ascii=False, separators=(",", ":"))
    hermes_payload = {
        "event_type": surface["topic"],
        "notify_payload": notify_payload,
    }
    body = json.dumps(hermes_payload, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    signature = hmac.new(config.hermes_route_secret.encode("utf-8"), body, sha256).hexdigest()
    request = urllib_request.Request(
        config.hermes_webhook_url,
        data=body,
        headers={
            "Content-Type": "application/json",
            "X-Webhook-Signature": signature,
            "X-Request-ID": str(surface["id"]),
            "User-Agent": "awiki-hermes-notify-adapter/1.0",
        },
        method="POST",
    )
    started = time.monotonic()
    try:
        with urllib_request.urlopen(request, timeout=config.hermes_timeout_seconds) as response:
            duration_ms = int((time.monotonic() - started) * 1000)
            LOGGER.info(
                "forwarded id=%s topic=%s status=%s duration_ms=%s",
                surface["id"],
                surface["topic"],
                response.status,
                duration_ms,
            )
            if 200 <= response.status < 300:
                return "notify"
            body_preview = response.read(512).decode("utf-8", errors="replace")
            raise RuntimeError(f"Hermes returned HTTP {response.status}: {body_preview}")
    except urllib_error.HTTPError as exc:
        body_preview = exc.read(1024).decode("utf-8", errors="replace")
        raise RuntimeError(f"Hermes returned HTTP {exc.code}: {body_preview}") from exc
    except urllib_error.URLError as exc:
        raise RuntimeError(f"failed to reach Hermes webhook: {exc}") from exc


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Hermes notify adapter with Notification Surface v1 ingress and host-event compatibility path."
    )
    parser.add_argument("--host", default="127.0.0.1", help="Listen host (default: 127.0.0.1)")
    parser.add_argument("--port", type=int, default=8765, help="Listen port (default: 8765)")
    parser.add_argument(
        "--notify-secret",
        default="",
        help="Shared secret for X-Notify-Signature verification",
    )
    parser.add_argument(
        "--notify-max-skew-seconds",
        type=int,
        default=300,
        help="Max allowed timestamp skew for incoming notify signatures (default: 300)",
    )
    parser.add_argument(
        "--max-body-bytes",
        type=int,
        default=262144,
        help="Max body size for incoming notify payloads (default: 262144)",
    )
    parser.add_argument(
        "--dedupe-ttl-seconds",
        type=int,
        default=3600,
        help="In-memory dedupe TTL by notify id (default: 3600)",
    )
    parser.add_argument(
        "--dedupe-max-entries",
        type=int,
        default=10000,
        help="Max in-memory dedupe entries (default: 10000)",
    )
    parser.add_argument(
        "--hermes-webhook-url",
        default="http://127.0.0.1:8644/webhooks/notify",
        help="Hermes webhook route URL (default: http://127.0.0.1:8644/webhooks/notify)",
    )
    parser.add_argument(
        "--hermes-route-secret",
        default="",
        help="HMAC secret for Hermes route signature",
    )
    parser.add_argument(
        "--hermes-timeout-seconds",
        type=float,
        default=10.0,
        help="HTTP timeout for Hermes webhook forwarding (default: 10)",
    )
    parser.add_argument(
        "--log-level",
        default="INFO",
        choices=["DEBUG", "INFO", "WARNING", "ERROR"],
        help="Log level (default: INFO)",
    )
    return parser


def main() -> None:
    parser = build_parser()
    args = parser.parse_args()

    logging.basicConfig(
        level=getattr(logging, args.log_level.upper(), logging.INFO),
        format="%(asctime)s %(levelname)s %(name)s %(message)s",
    )

    notify_secret = args.notify_secret.strip()
    hermes_route_secret = args.hermes_route_secret.strip()
    if not notify_secret:
        parser.error("--notify-secret is required")
    if not hermes_route_secret:
        parser.error("--hermes-route-secret is required")

    config = AdapterConfig(
        notify_secret=notify_secret,
        notify_max_skew_seconds=max(args.notify_max_skew_seconds, 1),
        max_body_bytes=max(args.max_body_bytes, 1024),
        dedupe_ttl_seconds=max(args.dedupe_ttl_seconds, 1),
        dedupe_max_entries=max(args.dedupe_max_entries, 128),
        hermes_webhook_url=args.hermes_webhook_url.strip(),
        hermes_route_secret=hermes_route_secret,
        hermes_timeout_seconds=max(args.hermes_timeout_seconds, 0.1),
    )

    server = HermesNotifyAdapterServer((args.host, args.port), config)
    LOGGER.info("Hermes notify adapter listening on http://%s:%s", args.host, args.port)
    LOGGER.info("Ingress endpoint: POST /notify")
    LOGGER.info("Compat endpoint: POST /notify/host-event")
    LOGGER.info("Health endpoint: GET /healthz")
    LOGGER.info("Hermes forward URL: %s", config.hermes_webhook_url)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        LOGGER.info("Shutting down Hermes notify adapter")
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
