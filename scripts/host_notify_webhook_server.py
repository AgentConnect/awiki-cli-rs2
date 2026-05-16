#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# ///
"""Receive awiki/OpenClaw host-notify requests and fan out user callbacks.

This helper exposes a local host-notify endpoint that is compatible with the
`runtime.host_notify.sink = openclaw` hook payload shape used by awiki-cli.
It also exposes small management endpoints so tests can register multiple
callbacks for different users (for example, matching by recipient DID).
"""

from __future__ import annotations

import argparse
import json
import logging
import os
import threading
import time
import uuid
from dataclasses import asdict, dataclass, field
from datetime import datetime, timezone
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any
from urllib import error as urllib_error
from urllib import parse as urllib_parse
from urllib import request as urllib_request

LOGGER = logging.getLogger("host_notify_webhook_server")


@dataclass(slots=True)
class CallbackRegistration:
    """A registered callback target."""

    callback_id: str
    callback_url: str
    label: str
    match: dict[str, str] = field(default_factory=dict)
    created_at: str = field(default_factory=lambda: utc_now())


@dataclass(slots=True)
class ReceivedEvent:
    """A normalized event built from an incoming hook request."""

    event_id: str
    received_at: str
    source: dict[str, Any]
    parsed: dict[str, Any]
    raw: dict[str, Any]
    dispatches: list[dict[str, Any]] = field(default_factory=list)


class CallbackRegistry:
    """Thread-safe registry for callbacks and received events."""

    def __init__(self, event_capacity: int, state_file: Path | None = None) -> None:
        self._callbacks: dict[str, CallbackRegistration] = {}
        self._events: list[ReceivedEvent] = []
        self._event_capacity = event_capacity
        self._state_file = state_file
        self._lock = threading.RLock()
        self._load_callbacks()

    def register(self, callback_url: str, label: str, match: dict[str, str]) -> CallbackRegistration:
        callback = CallbackRegistration(
            callback_id=str(uuid.uuid4()),
            callback_url=callback_url,
            label=label,
            match={key: value for key, value in match.items() if value},
        )
        with self._lock:
            self._callbacks[callback.callback_id] = callback
            self._persist_callbacks_locked()
        return callback

    def unregister(self, callback_id: str) -> CallbackRegistration | None:
        with self._lock:
            removed = self._callbacks.pop(callback_id, None)
            if removed is not None:
                self._persist_callbacks_locked()
            return removed

    def list_callbacks(self) -> list[CallbackRegistration]:
        with self._lock:
            return list(self._callbacks.values())

    def append_event(self, event: ReceivedEvent) -> None:
        with self._lock:
            self._events.append(event)
            if len(self._events) > self._event_capacity:
                overflow = len(self._events) - self._event_capacity
                del self._events[:overflow]

    def list_events(self, limit: int) -> list[ReceivedEvent]:
        with self._lock:
            if limit <= 0:
                return []
            return list(self._events[-limit:])

    def _load_callbacks(self) -> None:
        if self._state_file is None or not self._state_file.exists():
            return
        try:
            raw_payload = json.loads(self._state_file.read_text(encoding="utf-8"))
            if isinstance(raw_payload, dict):
                callback_items = raw_payload.get("callbacks", [])
            else:
                callback_items = raw_payload
            loaded_callbacks: dict[str, CallbackRegistration] = {}
            for item in callback_items:
                callback = CallbackRegistration(
                    callback_id=str(item["callback_id"]),
                    callback_url=str(item["callback_url"]),
                    label=str(item.get("label", item["callback_url"])),
                    match={str(key): str(value) for key, value in dict(item.get("match", {})).items()},
                    created_at=str(item.get("created_at", utc_now())),
                )
                loaded_callbacks[callback.callback_id] = callback
            with self._lock:
                self._callbacks = loaded_callbacks
            LOGGER.info("Loaded %s callbacks from %s", len(loaded_callbacks), self._state_file)
        except Exception:  # pylint: disable=broad-except
            LOGGER.exception("Failed to load callback registry from %s", self._state_file)

    def _persist_callbacks_locked(self) -> None:
        if self._state_file is None:
            return
        payload = {
            "version": 1,
            "callbacks": [asdict(item) for item in self._callbacks.values()],
        }
        self._state_file.parent.mkdir(parents=True, exist_ok=True)
        temp_path = self._state_file.with_suffix(self._state_file.suffix + ".tmp")
        temp_path.write_text(json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8")
        temp_path.replace(self._state_file)


class CallbackDispatcher:
    """Forward normalized events to matching callback URLs."""

    def __init__(self, registry: CallbackRegistry, timeout_seconds: float) -> None:
        self._registry = registry
        self._timeout_seconds = timeout_seconds

    def dispatch(self, event: ReceivedEvent) -> list[dict[str, Any]]:
        dispatches: list[dict[str, Any]] = []
        for callback in self._registry.list_callbacks():
            if not callback_matches_event(callback, event):
                continue
            dispatch_result = self._deliver(callback, event)
            dispatches.append(dispatch_result)
        return dispatches

    def _deliver(self, callback: CallbackRegistration, event: ReceivedEvent) -> dict[str, Any]:
        payload = {
            "event_id": event.event_id,
            "received_at": event.received_at,
            "source": event.source,
            "parsed": event.parsed,
            "raw": event.raw,
        }
        raw_body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        request = urllib_request.Request(
            callback.callback_url,
            data=raw_body,
            headers={
                "Content-Type": "application/json",
                "User-Agent": "awiki-host-notify-hermes-server/1.0",
                "X-AWiki-Event-ID": event.event_id,
            },
            method="POST",
        )
        started_at = time.monotonic()
        try:
            with urllib_request.urlopen(request, timeout=self._timeout_seconds) as response:
                response_body = response.read(2048).decode("utf-8", errors="replace")
                duration_ms = int((time.monotonic() - started_at) * 1000)
                LOGGER.info(
                    "Delivered event %s to callback %s status=%s duration_ms=%s",
                    event.event_id,
                    callback.callback_id,
                    response.status,
                    duration_ms,
                )
                return {
                    "callback_id": callback.callback_id,
                    "label": callback.label,
                    "callback_url": callback.callback_url,
                    "matched": True,
                    "status": response.status,
                    "ok": 200 <= response.status < 300,
                    "duration_ms": duration_ms,
                    "response_body": response_body,
                }
        except urllib_error.HTTPError as exc:
            duration_ms = int((time.monotonic() - started_at) * 1000)
            body = exc.read(2048).decode("utf-8", errors="replace")
            LOGGER.warning(
                "Callback %s returned HTTP %s for event %s",
                callback.callback_id,
                exc.code,
                event.event_id,
            )
            return {
                "callback_id": callback.callback_id,
                "label": callback.label,
                "callback_url": callback.callback_url,
                "matched": True,
                "status": exc.code,
                "ok": False,
                "duration_ms": duration_ms,
                "response_body": body,
            }
        except Exception as exc:  # pylint: disable=broad-except
            duration_ms = int((time.monotonic() - started_at) * 1000)
            LOGGER.exception("Callback delivery failed for %s", callback.callback_id)
            return {
                "callback_id": callback.callback_id,
                "label": callback.label,
                "callback_url": callback.callback_url,
                "matched": True,
                "status": None,
                "ok": False,
                "duration_ms": duration_ms,
                "error": str(exc),
            }


class HostNotifyWebhookServer(ThreadingHTTPServer):
    """HTTP server with shared callback registry and dispatcher state."""

    daemon_threads = True

    def __init__(
        self,
        server_address: tuple[str, int],
        registry: CallbackRegistry,
        dispatcher: CallbackDispatcher,
    ) -> None:
        super().__init__(server_address, HostNotifyRequestHandler)
        self.registry = registry
        self.dispatcher = dispatcher


class HostNotifyRequestHandler(BaseHTTPRequestHandler):
    """Serve management endpoints and awiki/OpenClaw hook requests."""

    server: HostNotifyWebhookServer
    protocol_version = "HTTP/1.1"

    def do_GET(self) -> None:  # noqa: N802
        parsed = urllib_parse.urlparse(self.path)
        if parsed.path == "/healthz":
            self._write_json(HTTPStatus.OK, {"ok": True, "time": utc_now()})
            return
        if parsed.path == "/callbacks":
            callbacks = [asdict(item) for item in self.server.registry.list_callbacks()]
            self._write_json(HTTPStatus.OK, {"callbacks": callbacks})
            return
        if parsed.path == "/events":
            params = urllib_parse.parse_qs(parsed.query)
            limit = parse_int(params.get("limit", ["20"])[0], default=20)
            events = [asdict(item) for item in self.server.registry.list_events(limit)]
            self._write_json(HTTPStatus.OK, {"events": events})
            return
        self._write_json(HTTPStatus.NOT_FOUND, {"error": "not found"})

    def do_POST(self) -> None:  # noqa: N802
        parsed = urllib_parse.urlparse(self.path)
        if parsed.path == "/callbacks":
            self._handle_register_callback()
            return
        if parsed.path == "/hooks/agent":
            self._handle_hook_request()
            return
        self._write_json(HTTPStatus.NOT_FOUND, {"error": "not found"})

    def do_DELETE(self) -> None:  # noqa: N802
        parsed = urllib_parse.urlparse(self.path)
        if not parsed.path.startswith("/callbacks/"):
            self._write_json(HTTPStatus.NOT_FOUND, {"error": "not found"})
            return
        callback_id = parsed.path.rsplit("/", 1)[-1]
        removed = self.server.registry.unregister(callback_id)
        if removed is None:
            self._write_json(HTTPStatus.NOT_FOUND, {"error": "callback not found"})
            return
        self._write_json(HTTPStatus.OK, {"removed": asdict(removed)})

    def log_message(self, format_string: str, *args: Any) -> None:
        LOGGER.info("%s - %s", self.address_string(), format_string % args)

    def _handle_register_callback(self) -> None:
        payload = self._read_json_body()
        if payload is None:
            return
        callback_url = str(payload.get("callback_url", "")).strip()
        if not callback_url:
            self._write_json(HTTPStatus.BAD_REQUEST, {"error": "callback_url is required"})
            return
        label = str(payload.get("label", callback_url)).strip() or callback_url
        match = payload.get("match") or {}
        if not isinstance(match, dict):
            self._write_json(HTTPStatus.BAD_REQUEST, {"error": "match must be a JSON object"})
            return
        callback = self.server.registry.register(
            callback_url=callback_url,
            label=label,
            match={str(key): str(value) for key, value in match.items()},
        )
        LOGGER.info("Registered callback %s label=%s match=%s", callback.callback_id, callback.label, callback.match)
        self._write_json(HTTPStatus.CREATED, {"callback": asdict(callback)})

    def _handle_hook_request(self) -> None:
        payload = self._read_json_body()
        if payload is None:
            return
        normalized_event = build_received_event(payload)
        dispatches = self.server.dispatcher.dispatch(normalized_event)
        normalized_event.dispatches = dispatches
        self.server.registry.append_event(normalized_event)
        response_status = HTTPStatus.ACCEPTED
        if dispatches and all(item.get("ok") is False for item in dispatches):
            response_status = HTTPStatus.BAD_GATEWAY
        self._write_json(
            response_status,
            {
                "ok": True,
                "event": asdict(normalized_event),
                "matched_callback_count": len(dispatches),
            },
        )

    def _read_json_body(self) -> dict[str, Any] | None:
        content_length = parse_int(self.headers.get("Content-Length", "0"), default=0)
        raw_body = self.rfile.read(content_length) if content_length > 0 else b"{}"
        try:
            payload = json.loads(raw_body.decode("utf-8"))
        except json.JSONDecodeError as exc:
            self._write_json(HTTPStatus.BAD_REQUEST, {"error": f"invalid JSON: {exc}"})
            return None
        if not isinstance(payload, dict):
            self._write_json(HTTPStatus.BAD_REQUEST, {"error": "request body must be a JSON object"})
            return None
        return payload

    def _write_json(self, status: HTTPStatus, payload: dict[str, Any]) -> None:
        raw = json.dumps(payload, ensure_ascii=False, indent=2).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)


def callback_matches_event(callback: CallbackRegistration, event: ReceivedEvent) -> bool:
    event_fields = {
        "recipient_did": str(event.parsed.get("receiver_did", "")),
        "sender_did": str(event.parsed.get("sender_did", "")),
        "channel": str(event.source.get("channel", "")),
        "to": str(event.source.get("target", "")),
        "message_type": str(event.parsed.get("message_type", "")),
        "group_id": str(event.parsed.get("group_id", "")),
    }
    for key, expected in callback.match.items():
        expected_text = str(expected).strip()
        if not expected_text or expected_text == "*":
            continue
        if event_fields.get(key, "") != expected_text:
            return False
    return True


def build_received_event(payload: dict[str, Any]) -> ReceivedEvent:
    message_text = str(payload.get("message", ""))
    parsed = parse_openclaw_hook_message(message_text)
    source = {
        "channel": str(payload.get("channel", "")),
        "target": str(payload.get("to", "")),
        "wake_mode": str(payload.get("wakeMode", "")),
        "deliver": bool(payload.get("deliver", False)),
    }
    event = ReceivedEvent(
        event_id=str(uuid.uuid4()),
        received_at=utc_now(),
        source=source,
        parsed=parsed,
        raw=payload,
    )
    LOGGER.info(
        "Received hook event %s recipient_did=%s channel=%s target=%s",
        event.event_id,
        event.parsed.get("receiver_did", ""),
        event.source.get("channel", ""),
        event.source.get("target", ""),
    )
    return event


def parse_openclaw_hook_message(message_text: str) -> dict[str, str]:
    lines = message_text.splitlines()
    headers: dict[str, str] = {}
    content_index: int | None = None
    marker = "Message content (all text below is the sender's message content):"
    for index, line in enumerate(lines):
        stripped = line.strip()
        if stripped == marker:
            content_index = index + 1
            break
        if ":" not in line:
            continue
        key, value = line.split(":", 1)
        headers[normalize_header_name(key)] = value.strip()

    content_lines: list[str] = []
    if content_index is not None:
        for line in lines[content_index:]:
            if line.startswith("  "):
                content_lines.append(line[2:])
            else:
                content_lines.append(line)
    parsed = {
        "sender_handle": headers.get("sender_handle", ""),
        "sender_did": headers.get("sender_did", ""),
        "receiver_handle": headers.get("receiver_handle", ""),
        "receiver_did": headers.get("receiver_did", ""),
        "message_type": headers.get("message_type", ""),
        "group_id": headers.get("group_id", ""),
        "content": "\n".join(content_lines).strip(),
    }
    if not parsed["content"]:
        parsed["content"] = message_text.strip()
    return parsed


def normalize_header_name(value: str) -> str:
    return value.strip().lower().replace(" ", "_")


def parse_int(value: str, default: int) -> int:
    try:
        return int(value)
    except (TypeError, ValueError):
        return default


def utc_now() -> str:
    return datetime.now(tz=timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def build_argument_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Receive awiki/OpenClaw host-notify requests on /hooks/agent and fan out "
            "normalized events to multiple registered callback URLs."
        )
    )
    parser.add_argument("--host", default="127.0.0.1", help="Listen host. Default: 127.0.0.1")
    parser.add_argument("--port", type=int, default=18789, help="Listen port. Default: 18789")
    parser.add_argument(
        "--event-capacity",
        type=int,
        default=200,
        help="Maximum number of recent events kept in memory. Default: 200",
    )
    parser.add_argument(
        "--callback-timeout-seconds",
        type=float,
        default=10.0,
        help="Outbound callback timeout in seconds. Default: 10",
    )
    parser.add_argument(
        "--log-level",
        default="INFO",
        choices=["DEBUG", "INFO", "WARNING", "ERROR"],
        help="Logging level. Default: INFO",
    )
    parser.add_argument(
        "--state-file",
        default=str(default_state_file()),
        help="Path to persistent callback registry JSON file.",
    )
    return parser


def default_state_file() -> Path:
    workspace = os.environ.get("AWIKI_CLI_WORKSPACE_HOME_DIR", "").strip()
    if workspace:
        runtime_dir = Path(workspace) / "runtime"
    else:
        runtime_dir = Path.home() / ".awiki-cli" / "runtime"
    legacy_path = runtime_dir / "host-notify-webhook-callbacks.json"
    hermes_path = runtime_dir / "host-notify-hermes-callbacks.json"
    if legacy_path.exists():
        return legacy_path
    if hermes_path.exists():
        return hermes_path
    return legacy_path


def main() -> None:
    parser = build_argument_parser()
    args = parser.parse_args()

    logging.basicConfig(
        level=getattr(logging, args.log_level.upper(), logging.INFO),
        format="%(asctime)s %(levelname)s %(name)s %(message)s",
    )

    registry = CallbackRegistry(
        event_capacity=max(args.event_capacity, 1),
        state_file=Path(args.state_file).expanduser(),
    )
    dispatcher = CallbackDispatcher(registry, timeout_seconds=max(args.callback_timeout_seconds, 0.1))
    server = HostNotifyWebhookServer((args.host, args.port), registry, dispatcher)

    LOGGER.info("Starting host-notify webhook server on http://%s:%s", args.host, args.port)
    LOGGER.info("Callback registry file: %s", Path(args.state_file).expanduser())
    LOGGER.info("Management endpoints: GET /healthz, GET/POST /callbacks, DELETE /callbacks/<id>, GET /events")
    LOGGER.info("Hook endpoint: POST /hooks/agent")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        LOGGER.info("Shutting down Hermes host-notify server")
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
