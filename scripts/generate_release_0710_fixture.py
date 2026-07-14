"""Generate a redacted release/0710 local-state fixture with the released daemon.

[INPUT]: An exact release/0710 daemon artifact, its 40-character source ref,
and an output directory.
[OUTPUT]: A schema-27 SQLite fixture plus a provenance and conservation
manifest that contain only synthetic identities and message content.
[POS]: Release evidence generator for Canonical Conversation schema migration.

[PROTOCOL]:
1. Logic changes must update this header and fixture documentation.
2. The generator must run with an isolated state root and never read live data.
3. Secrets created by daemon init-state must never enter the output directory.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import sqlite3
import subprocess
import tempfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable


SOURCE_SCHEMA_VERSION = 27
FIXED_TIME = "2026-07-14T00:00:00Z"
OWNER_ID = "fixture-owner-id"
OWNER_DID = "did:wba:awiki.info:fixture-owner:e1_fixture_owner"
PREVIOUS_OWNER_DID = "did:wba:awiki.info:fixture-owner:e1_fixture_owner_old"
PEER_USER_ID = "fixture-peer-user"
PEER_HANDLE = "fixture-peer.awiki.info"
PEER_DID = "did:wba:awiki.info:fixture-peer:e1_fixture_peer"
PREVIOUS_PEER_DID = "did:wba:awiki.info:fixture-peer:e1_fixture_peer_old"
GROUP_ID = "fixture-group-local"
GROUP_DID = "did:wba:awiki.info:groups:fixture-group:e1_fixture_group"
EMPTY_GROUP_ID = "fixture-empty-group-local"
EMPTY_GROUP_DID = "did:wba:awiki.info:groups:fixture-empty:e1_fixture_empty"
GROUP_MESSAGE_ID = f"{GROUP_DID}:11"


def _sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _sha256_json(value: Any) -> str:
    payload = json.dumps(
        value,
        ensure_ascii=True,
        separators=(",", ":"),
        sort_keys=True,
    ).encode("utf-8")
    return hashlib.sha256(payload).hexdigest()


def _schema_fingerprint(connection: sqlite3.Connection) -> str:
    rows = connection.execute(
        """
        SELECT type, name, COALESCE(sql, '')
        FROM sqlite_schema
        WHERE name NOT LIKE 'sqlite_%'
        ORDER BY type, name
        """
    ).fetchall()
    digest = hashlib.sha256()
    for kind, name, sql in rows:
        digest.update(kind.encode("utf-8"))
        digest.update(b"\0")
        digest.update(name.encode("utf-8"))
        digest.update(b"\0")
        for token in sql.split():
            digest.update(token.encode("utf-8"))
            digest.update(b" ")
        digest.update(b"\n")
    return f"sha256:{digest.hexdigest()}"


def _direct_conversation_id() -> str:
    source = f"user:{PEER_USER_ID}\nhandle:{PEER_HANDLE}".encode("utf-8")
    return f"dm:peer-scope:v1:{hashlib.sha256(source).hexdigest()}"


def _insert_fixture_rows(connection: sqlite3.Connection) -> None:
    direct_conversation_id = _direct_conversation_id()
    legacy_direct_conversation_id = f"dm:{PREVIOUS_PEER_DID}"
    group_conversation_id = f"group:{GROUP_ID}"
    empty_group_conversation_id = f"group:{EMPTY_GROUP_ID}"

    connection.executescript(
        f"""
        BEGIN IMMEDIATE;

        INSERT INTO identity_did_history
          (owner_identity_id, did, status, first_seen_at, last_seen_at, metadata)
        VALUES
          ('{OWNER_ID}', '{PREVIOUS_OWNER_DID}', 'historical',
           '{FIXED_TIME}', '{FIXED_TIME}', '{{"fixture":true}}'),
          ('{OWNER_ID}', '{OWNER_DID}', 'current',
           '{FIXED_TIME}', '{FIXED_TIME}', '{{"fixture":true}}');

        INSERT INTO contacts
          (owner_identity_id, owner_did, did, name, handle, nick_name,
           followed, messaged, note, first_seen_at, last_seen_at, metadata)
        VALUES
          ('{OWNER_ID}', '{OWNER_DID}', '{PREVIOUS_PEER_DID}', 'Fixture Peer',
           '{PEER_HANDLE}', 'Fixture Nickname', 1, 1, 'Fixture Note',
           '{FIXED_TIME}', '{FIXED_TIME}', '{{"fixture":true}}'),
          ('{OWNER_ID}', '{OWNER_DID}', '{PEER_DID}', 'Fixture Peer',
           '{PEER_HANDLE}', 'Fixture Nickname', 1, 1, 'Fixture Note',
           '{FIXED_TIME}', '{FIXED_TIME}', '{{"fixture":true}}');

        INSERT INTO contact_handle_bindings
          (owner_identity_id, owner_did, handle, did, is_current,
           first_seen_at, last_seen_at, source_type, metadata)
        VALUES
          ('{OWNER_ID}', '{OWNER_DID}', '{PEER_HANDLE}',
           '{PREVIOUS_PEER_DID}', 0, '{FIXED_TIME}', '{FIXED_TIME}',
           'fixture', '{{"binding_generation":"1"}}'),
          ('{OWNER_ID}', '{OWNER_DID}', '{PEER_HANDLE}', '{PEER_DID}', 1,
           '{FIXED_TIME}', '{FIXED_TIME}', 'fixture',
           '{{"binding_generation":"2"}}');

        INSERT INTO direct_peer_routes
          (owner_identity_id, conversation_id, peer_user_id, full_handle,
           current_did, updated_at)
        VALUES
          ('{OWNER_ID}', '{direct_conversation_id}', '{PEER_USER_ID}',
           '{PEER_HANDLE}', '{PEER_DID}', '{FIXED_TIME}');

        INSERT INTO groups
          (owner_identity_id, owner_did, group_id, group_did, name,
           group_owner_did, group_owner_handle, my_role, membership_status,
           member_count, last_synced_seq, last_read_seq, stored_at, metadata)
        VALUES
          ('{OWNER_ID}', '{OWNER_DID}', '{GROUP_ID}', '{GROUP_DID}',
           'Fixture Group', '{OWNER_DID}', 'fixture-owner.awiki.info',
           'owner', 'active', 2, 11, 10, '{FIXED_TIME}',
           '{{"fixture":true}}'),
          ('{OWNER_ID}', '{OWNER_DID}', '{EMPTY_GROUP_ID}',
           '{EMPTY_GROUP_DID}', 'Fixture Empty Group', '{OWNER_DID}',
           'fixture-owner.awiki.info', 'owner', 'active', 1, 0, 0,
           '{FIXED_TIME}', '{{"fixture":true}}');

        INSERT INTO group_members
          (owner_identity_id, owner_did, group_id, user_id, member_did,
           member_handle, anchor_kind, anchor_value,
           handle_binding_generation, role, status, joined_at,
           sent_message_count, last_synced_at, metadata)
        VALUES
          ('{OWNER_ID}', '{OWNER_DID}', '{GROUP_ID}', '{PEER_USER_ID}',
           '{PEER_DID}', '{PEER_HANDLE}', 'handle', '{PEER_HANDLE}', '2',
           'member', 'active', '{FIXED_TIME}', 1, '{FIXED_TIME}',
           '{{"fixture":true}}'),
          ('{OWNER_ID}', '{OWNER_DID}', '{EMPTY_GROUP_ID}', '{OWNER_ID}',
           '{OWNER_DID}', 'fixture-owner.awiki.info', 'handle',
           'fixture-owner.awiki.info', '1', 'owner', 'active',
           '{FIXED_TIME}', 0, '{FIXED_TIME}', '{{"fixture":true}}');

        INSERT INTO conversation_registry
          (owner_identity_id, owner_did, conversation_id, thread_kind,
           thread_id, activity_at, created_at, updated_at, is_active)
        VALUES
          ('{OWNER_ID}', '{OWNER_DID}', '{direct_conversation_id}', 'direct',
           '{direct_conversation_id}', '{FIXED_TIME}', '{FIXED_TIME}',
           '{FIXED_TIME}', 1),
          ('{OWNER_ID}', '{OWNER_DID}', '{legacy_direct_conversation_id}',
           'thread', '{legacy_direct_conversation_id}', '{FIXED_TIME}',
           '{FIXED_TIME}', '{FIXED_TIME}', 1),
          ('{OWNER_ID}', '{OWNER_DID}', '{group_conversation_id}', 'group',
           '{GROUP_ID}', '{FIXED_TIME}', '{FIXED_TIME}', '{FIXED_TIME}', 1),
          ('{OWNER_ID}', '{OWNER_DID}', '{empty_group_conversation_id}',
           'group', '{EMPTY_GROUP_ID}', '{FIXED_TIME}', '{FIXED_TIME}',
           '{FIXED_TIME}', 1);

        INSERT INTO messages
          (msg_id, owner_identity_id, owner_did, conversation_id, thread_id,
           direction, sender_did, receiver_did, group_id, group_did,
           content_type, content, server_seq, sent_at, stored_at, is_e2ee,
           is_read, sender_name, metadata, mentions_current_user)
        VALUES
          ('fixture-direct-message-1', '{OWNER_ID}', '{OWNER_DID}',
           '{legacy_direct_conversation_id}', '{legacy_direct_conversation_id}',
           0, '{PREVIOUS_PEER_DID}', '{OWNER_DID}', NULL, NULL,
           'application/awiki-fixture', 'fixture-direct-ciphertext-v1', 7,
           '{FIXED_TIME}', '{FIXED_TIME}', 1, 0, 'Fixture Peer',
           '{{"aad_fingerprint":"fixture-aad-v1","fixture":true}}', 0),
          ('{GROUP_MESSAGE_ID}', '{OWNER_ID}', '{OWNER_DID}',
           '{group_conversation_id}', '{group_conversation_id}', 0,
           '{PEER_DID}', '{OWNER_DID}', '{GROUP_ID}', '{GROUP_DID}',
           'application/awiki-fixture', 'fixture-group-ciphertext-v1', 11,
           '{FIXED_TIME}', '{FIXED_TIME}', 1, 1, 'Fixture Peer',
           '{{"aad_fingerprint":"fixture-group-aad-v1","fixture":true}}',
           1);

        INSERT INTO conversation_summaries
          (owner_identity_id, owner_did, conversation_id, thread_id,
           message_count, unread_count, unread_mention_count,
           first_unread_mention_message_id, last_message_id, last_message_at,
           last_content, last_content_type, last_sender_did, last_sender_name,
           group_id, group_did, updated_at)
        VALUES
          ('{OWNER_ID}', '{OWNER_DID}', '{legacy_direct_conversation_id}',
           '{legacy_direct_conversation_id}', 1, 1, 0, NULL,
           'fixture-direct-message-1', '{FIXED_TIME}',
           'fixture-direct-ciphertext-v1', 'application/awiki-fixture',
           '{PREVIOUS_PEER_DID}', 'Fixture Peer', NULL, NULL, '{FIXED_TIME}'),
          ('{OWNER_ID}', '{OWNER_DID}', '{group_conversation_id}',
           '{group_conversation_id}', 1, 0, 1, '{GROUP_MESSAGE_ID}',
           '{GROUP_MESSAGE_ID}', '{FIXED_TIME}',
           'fixture-group-ciphertext-v1', 'application/awiki-fixture',
           '{PEER_DID}', 'Fixture Peer', '{GROUP_ID}', '{GROUP_DID}',
           '{FIXED_TIME}');

        INSERT INTO thread_read_state
          (owner_identity_id, owner_did, thread_scope, thread_id,
           conversation_id, read_watermark_message_id, read_watermark_seq,
           read_watermark_at, pending_remote_ack, updated_at)
        VALUES
          ('{OWNER_ID}', '{OWNER_DID}', 'direct',
           '{legacy_direct_conversation_id}', '{legacy_direct_conversation_id}',
           'fixture-direct-message-1', '7', '{FIXED_TIME}', 1, '{FIXED_TIME}'),
          ('{OWNER_ID}', '{OWNER_DID}', 'group', '{group_conversation_id}',
           '{group_conversation_id}', '{GROUP_MESSAGE_ID}', '11',
           '{FIXED_TIME}', 0, '{FIXED_TIME}');

        INSERT INTO e2ee_outbox
          (outbox_id, owner_identity_id, owner_did, peer_did, session_id,
           original_type, plaintext, local_status, attempt_count, metadata,
           created_at, updated_at)
        VALUES
          ('fixture-outbox-1', '{OWNER_ID}', '{OWNER_DID}', '{PEER_DID}',
           'fixture-session-1', 'application/awiki-fixture',
           'fixture-outbox-payload-v1', 'queued', 0,
           '{{"fixture":true}}', '{FIXED_TIME}', '{FIXED_TIME}');

        INSERT INTO group_rebind_outbox
          (job_id, owner_identity_id, group_did, member_handle,
           previous_member_did, new_member_did, binding_generation, phase,
           group_state_ref_json, attempt_count, created_at, updated_at)
        VALUES
          ('fixture-rebind-1', '{OWNER_ID}', '{GROUP_DID}', '{PEER_HANDLE}',
           '{PREVIOUS_PEER_DID}', '{PEER_DID}', '2', 'pending',
           '{{"event_id":"fixture-event-1","fixture":true}}', 0,
           '{FIXED_TIME}', '{FIXED_TIME}');

        INSERT INTO group_rebind_p6_jobs
          (job_id, owner_identity_id, group_did, event_id, member_handle,
           previous_member_did, new_member_did, binding_generation,
           group_state_ref_json, phase, attempt_count, created_at, updated_at)
        VALUES
          ('fixture-rebind-p6-1', '{OWNER_ID}', '{GROUP_DID}',
           'fixture-event-1', '{PEER_HANDLE}', '{PREVIOUS_PEER_DID}',
           '{PEER_DID}', '2',
           '{{"event_id":"fixture-event-1","fixture":true}}',
           'awaiting_add', 0, '{FIXED_TIME}', '{FIXED_TIME}');

        INSERT INTO relationship_events
          (event_id, owner_identity_id, owner_did, target_did, target_handle,
           event_type, source_type, reason, status, created_at, updated_at,
           metadata)
        VALUES
          ('fixture-relationship-1', '{OWNER_ID}', '{OWNER_DID}',
           '{PEER_DID}', '{PEER_HANDLE}', 'follow', 'fixture',
           'fixture-reason', 'completed', '{FIXED_TIME}', '{FIXED_TIME}',
           '{{"fixture":true}}');

        INSERT INTO sync_state
          (owner_identity_id, owner_did, scope, checkpoint_kind, event_seq,
           updated_at, metadata_json)
        VALUES
          ('{OWNER_ID}', '{OWNER_DID}', 'messages', 'event_seq', '11',
           '{FIXED_TIME}', '{{"fixture":true}}');

        COMMIT;
        """
    )


def _rows(
    connection: sqlite3.Connection,
    query: str,
    parameters: Iterable[Any] = (),
) -> list[list[Any]]:
    return [list(row) for row in connection.execute(query, tuple(parameters))]


def _build_oracles(connection: sqlite3.Connection) -> dict[str, Any]:
    tables = [
        "contacts",
        "contact_handle_bindings",
        "conversation_registry",
        "conversation_summaries",
        "direct_peer_routes",
        "e2ee_outbox",
        "group_members",
        "group_rebind_outbox",
        "group_rebind_p6_jobs",
        "groups",
        "identity_did_history",
        "messages",
        "relationship_events",
        "sync_state",
        "thread_read_state",
    ]
    counts = {
        table: connection.execute(f"SELECT COUNT(*) FROM {table}").fetchone()[0]
        for table in tables
    }
    fingerprints = {
        "message_ids": _sha256_json(
            _rows(connection, "SELECT msg_id FROM messages ORDER BY msg_id")
        ),
        "message_wire_facts": _sha256_json(
            _rows(
                connection,
                """
                SELECT msg_id, conversation_id, thread_id, sender_did,
                       receiver_did, group_id, group_did, server_seq, is_e2ee,
                       metadata
                FROM messages ORDER BY msg_id
                """,
            )
        ),
        "outbox_facts": _sha256_json(
            _rows(
                connection,
                """
                SELECT outbox_id, peer_did, session_id, original_type,
                       local_status, attempt_count, created_at
                FROM e2ee_outbox ORDER BY outbox_id
                """,
            )
        ),
        "read_facts": _sha256_json(
            _rows(
                connection,
                """
                SELECT thread_scope, thread_id, conversation_id,
                       read_watermark_message_id, read_watermark_seq,
                       pending_remote_ack
                FROM thread_read_state ORDER BY thread_scope, thread_id
                """,
            )
        ),
        "membership_facts": _sha256_json(
            _rows(
                connection,
                """
                SELECT group_id, user_id, member_did, member_handle,
                       anchor_kind, anchor_value, handle_binding_generation,
                       status
                FROM group_members ORDER BY group_id, user_id
                """,
            )
        ),
        "rebind_facts": _sha256_json(
            _rows(
                connection,
                """
                SELECT job_id, group_did, member_handle, previous_member_did,
                       new_member_did, binding_generation, phase
                FROM group_rebind_outbox ORDER BY job_id
                """,
            )
        ),
        "empty_conversations": _sha256_json(
            _rows(
                connection,
                """
                SELECT registry.conversation_id
                FROM conversation_registry AS registry
                LEFT JOIN messages
                  ON messages.owner_identity_id = registry.owner_identity_id
                 AND messages.conversation_id = registry.conversation_id
                GROUP BY registry.owner_identity_id, registry.conversation_id
                HAVING COUNT(messages.msg_id) = 0
                ORDER BY registry.conversation_id
                """,
            )
        ),
    }
    return {"counts": counts, "fingerprints": fingerprints}


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--daemon-binary", type=Path, required=True)
    parser.add_argument("--source-ref", required=True)
    parser.add_argument("--artifact-version", required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    return parser.parse_args()


def main() -> None:
    args = _parse_args()
    source_ref = args.source_ref.lower()
    if not re.fullmatch(r"[0-9a-f]{40}", source_ref):
        raise SystemExit("--source-ref must be an exact 40-character commit SHA")
    daemon_binary = args.daemon_binary.resolve()
    if not daemon_binary.is_file():
        raise SystemExit("--daemon-binary must point to an existing file")

    output_dir = args.output_dir.resolve()
    output_dir.mkdir(parents=True, exist_ok=True)
    fixture_path = output_dir / "local-state.sqlite"
    manifest_path = output_dir / "manifest.json"

    with tempfile.TemporaryDirectory(prefix="awiki-release-0710-fixture-") as root:
        state_root = Path(root) / "state"
        completed = subprocess.run(
            [
                str(daemon_binary),
                "init-state",
                "--state-root",
                str(state_root),
            ],
            check=True,
            capture_output=True,
            text=True,
        )
        init_report = json.loads(completed.stdout)
        source_path = Path(init_report["im_core_sqlite_path"])
        if init_report["im_core_schema_version"] != SOURCE_SCHEMA_VERSION:
            raise SystemExit("daemon artifact did not initialize schema 27")

        connection = sqlite3.connect(source_path)
        try:
            _insert_fixture_rows(connection)
            integrity = connection.execute("PRAGMA integrity_check").fetchone()[0]
            if integrity != "ok":
                raise SystemExit("generated fixture failed integrity_check")
            connection.execute("PRAGMA wal_checkpoint(TRUNCATE)")
            connection.execute("PRAGMA journal_mode=DELETE")
            connection.execute("VACUUM")
            schema_fingerprint = _schema_fingerprint(connection)
            oracles = _build_oracles(connection)
        finally:
            connection.close()

        shutil.copyfile(source_path, fixture_path)

    manifest = {
        "formatVersion": 1,
        "generatedAt": datetime.now(timezone.utc).replace(microsecond=0).isoformat(),
        "generator": {
            "path": "scripts/generate_release_0710_fixture.py",
            "sha256": _sha256_file(Path(__file__).resolve()),
        },
        "sourceArtifact": {
            "component": "awiki-deamon",
            "version": args.artifact_version,
            "sourceRef": source_ref,
            "sha256": _sha256_file(daemon_binary),
        },
        "sourceSchema": {
            "version": SOURCE_SCHEMA_VERSION,
            "fingerprint": schema_fingerprint,
        },
        "fixture": {
            "file": fixture_path.name,
            "sha256": _sha256_file(fixture_path),
            "containsOnlySyntheticData": True,
        },
        "oracles": oracles,
    }
    manifest_path.write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(json.dumps(manifest, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
