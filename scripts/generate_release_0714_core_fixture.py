"""Generate the release/0714 schema-36 Core migration fixture.

[INPUT]: A locally built daemon from the exact historical source ref and an
output directory.
[OUTPUT]: A schema-36 SQLite fixture plus provenance, integrity, and
conservation metadata containing only deterministic synthetic rows.

The generator initializes a fresh isolated state root. It never reads a live
database, identity, message, credential, key, token, or service response.
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
from pathlib import Path
from typing import Any


SOURCE_SCHEMA_VERSION = 36
SOURCE_REF = "e2cf7f4cd00debba5353980e6d33c3ba682cdd0c"
ANP_SOURCE_REF = "59475cf76b23838a911a7263287ce6b7399d8e02"
ARTIFACT_VERSION = "0.1.91"
FIXED_TIME = "2026-07-14T00:00:00Z"
OWNER_ID = "fixture-owner-id"
OWNER_DID = "did:wba:fixture-owner.fixture.invalid"
PEER_DID = "did:wba:fixture-peer.fixture.invalid"
GROUP_DID = "did:wba:fixture-group.fixture.invalid"

SNAPSHOT_QUERIES = (
    "SELECT owner_identity_id,current_did,identity_generation,device_auth_generation "
    "FROM identity_account_bindings ORDER BY owner_identity_id",
    "SELECT conversation_id,thread_kind,thread_id,lifecycle_state,resolution_state "
    "FROM conversation_registry ORDER BY conversation_id",
    "SELECT conversation_id,CAST(message_count AS TEXT),CAST(unread_count AS TEXT),"
    "CAST(unread_mention_count AS TEXT),COALESCE(first_unread_mention_message_id,'') "
    "FROM conversation_summaries ORDER BY conversation_id",
    "SELECT msg_id,conversation_id,CAST(server_seq AS TEXT),CAST(is_read AS TEXT),"
    "CAST(mentions_current_user AS TEXT) FROM messages ORDER BY msg_id",
    "SELECT conversation_id,peer_user_id,full_handle,current_did "
    "FROM direct_peer_routes ORDER BY conversation_id",
    "SELECT outbox_id,local_status,CAST(attempt_count AS TEXT),plaintext "
    "FROM e2ee_outbox ORDER BY outbox_id",
    "SELECT sync_subject_id,scope,checkpoint_kind,event_seq "
    "FROM sync_state ORDER BY sync_subject_id,scope,checkpoint_kind",
    "SELECT thread_kind,thread_id,message_id,content "
    "FROM attachment_manifest_cache ORDER BY thread_kind,thread_id,message_id",
    "SELECT job_id,phase,CAST(attempt_count AS TEXT),group_state_ref_json "
    "FROM group_rebind_outbox ORDER BY job_id",
)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def schema_fingerprint(connection: sqlite3.Connection) -> str:
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
        digest.update(kind.encode())
        digest.update(b"\0")
        digest.update(name.encode())
        digest.update(b"\0")
        for token in sql.split():
            digest.update(token.encode())
            digest.update(b" ")
        digest.update(b"\n")
    return f"sha256:{digest.hexdigest()}"


def conservation_digest(connection: sqlite3.Connection) -> str:
    digest = hashlib.sha256()
    for query in SNAPSHOT_QUERIES:
        digest.update(query.encode())
        for row in connection.execute(query):
            for value in row:
                encoded = str(value).encode()
                digest.update(len(encoded).to_bytes(8, "big"))
                digest.update(encoded)
    return digest.hexdigest()


def rows(connection: sqlite3.Connection, query: str) -> list[list[Any]]:
    return [list(row) for row in connection.execute(query)]


def assert_empty_user_tables(connection: sqlite3.Connection) -> None:
    populated = []
    names = connection.execute(
        """
        SELECT name FROM sqlite_schema
        WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
        ORDER BY name
        """
    )
    for (name,) in names:
        if connection.execute(f'SELECT COUNT(*) FROM "{name}"').fetchone()[0]:
            populated.append(name)
    if populated:
        raise SystemExit(f"historical daemon initialized non-empty tables: {populated}")


def insert_synthetic_rows(connection: sqlite3.Connection) -> None:
    connection.executescript(
        f"""
        BEGIN IMMEDIATE;

        INSERT INTO identity_account_bindings
          (owner_identity_id, account_id, handle_scope, current_did, device_id,
           identity_generation, device_auth_generation, created_at, updated_at)
        VALUES
          ('{OWNER_ID}', 'fixture-account-id', 'fixture.invalid', '{OWNER_DID}',
           'fixture-device-id', '1', '1', 1786665600, 1786665600);

        INSERT INTO conversation_registry
          (owner_identity_id, owner_did, conversation_id, thread_kind,
           thread_id, activity_at, created_at, updated_at, is_active,
           lifecycle_state, resolution_state)
        VALUES
          ('{OWNER_ID}', '{OWNER_DID}', 'fixture-direct-conversation', 'direct',
           'fixture-direct-thread', '{FIXED_TIME}', '{FIXED_TIME}',
           '{FIXED_TIME}', 1, 'active', 'resolved'),
          ('{OWNER_ID}', '{OWNER_DID}', 'fixture-group-conversation', 'group',
           'fixture-group-thread', '{FIXED_TIME}', '{FIXED_TIME}',
           '{FIXED_TIME}', 1, 'active', 'resolved');

        INSERT INTO messages
          (msg_id, owner_identity_id, owner_did, conversation_id,
           wire_thread_kind, wire_thread_ref, wire_identity_resolution_state,
           thread_id, direction, sender_did, receiver_did, group_id, group_did,
           content_type, content, server_seq, hydration_state, sent_at,
           stored_at, is_e2ee, is_read, sender_name, metadata,
           mentions_current_user)
        VALUES
          ('fixture-direct-message', '{OWNER_ID}', '{OWNER_DID}',
           'fixture-direct-conversation', 'direct', 'fixture-direct-thread',
           'resolved', 'fixture-direct-thread', 0, '{PEER_DID}', '{OWNER_DID}',
           NULL, NULL, 'application/awiki-fixture',
           'fixture-direct-ciphertext', 7, 'hydrated', '{FIXED_TIME}',
           '{FIXED_TIME}', 1, 0, 'Fixture Peer', '{{"fixture":true}}', 0),
          ('fixture-group-message', '{OWNER_ID}', '{OWNER_DID}',
           'fixture-group-conversation', 'group', 'fixture-group-thread',
           'resolved', 'fixture-group-thread', 0, '{PEER_DID}', '{OWNER_DID}',
           'fixture-group-id', '{GROUP_DID}', 'application/awiki-fixture',
           'fixture-group-ciphertext', 11, 'hydrated', '{FIXED_TIME}',
           '{FIXED_TIME}', 1, 1, 'Fixture Peer', '{{"fixture":true}}', 1);

        INSERT INTO conversation_summaries
          (owner_identity_id, owner_did, conversation_id, thread_id,
           message_count, unread_count, unread_mention_count,
           first_unread_mention_message_id, last_message_id, last_message_at,
           last_content, last_content_type, last_sender_did, last_sender_name,
           group_id, group_did, updated_at)
        VALUES
          ('{OWNER_ID}', '{OWNER_DID}', 'fixture-direct-conversation',
           'fixture-direct-thread', 1, 1, 0, NULL,
           'fixture-direct-message', '{FIXED_TIME}',
           'fixture-direct-ciphertext', 'application/awiki-fixture',
           '{PEER_DID}', 'Fixture Peer', NULL, NULL, '{FIXED_TIME}'),
          ('{OWNER_ID}', '{OWNER_DID}', 'fixture-group-conversation',
           'fixture-group-thread', 1, 0, 1, 'fixture-group-message',
           'fixture-group-message', '{FIXED_TIME}',
           'fixture-group-ciphertext', 'application/awiki-fixture',
           '{PEER_DID}', 'Fixture Peer', 'fixture-group-id', '{GROUP_DID}',
           '{FIXED_TIME}');

        INSERT INTO direct_peer_routes
          (owner_identity_id, conversation_id, peer_persona_id,
           authority_namespace, peer_user_id, full_handle, current_did,
           updated_at)
        VALUES
          ('{OWNER_ID}', 'fixture-direct-conversation', 'fixture-peer-persona',
           'fixture.invalid', 'fixture-peer-user', 'peer.fixture.invalid',
           '{PEER_DID}', '{FIXED_TIME}');

        INSERT INTO e2ee_outbox
          (outbox_id, owner_identity_id, owner_did, peer_did, session_id,
           original_type, plaintext, local_status, attempt_count, metadata,
           created_at, updated_at)
        VALUES
          ('fixture-outbox', '{OWNER_ID}', '{OWNER_DID}', '{PEER_DID}',
           'fixture-session', 'application/awiki-fixture',
           'fixture-outbox-plaintext', 'queued', 0, '{{"fixture":true}}',
           '{FIXED_TIME}', '{FIXED_TIME}');

        INSERT INTO sync_state
          (owner_identity_id, sync_subject_id, scope, checkpoint_kind,
           event_seq, updated_at, metadata_json)
        VALUES
          ('{OWNER_ID}', 'fixture-subject', 'messages', 'event_seq', '11',
           '{FIXED_TIME}', '{{"fixture":true}}');

        INSERT INTO attachment_manifest_cache
          (owner_identity_id, owner_did, thread_kind, thread_id, message_id,
           wire_message_id, sender_did, message_security_profile, content,
           stored_at)
        VALUES
          ('{OWNER_ID}', '{OWNER_DID}', 'direct', 'fixture-direct-thread',
           'fixture-direct-message', 'fixture-direct-message', '{PEER_DID}',
           'e2ee', '{{"fixture":true}}', '{FIXED_TIME}');

        INSERT INTO group_rebind_outbox
          (job_id, owner_identity_id, group_did, member_handle,
           previous_member_did, new_member_did, binding_generation, phase,
           group_state_ref_json, attempt_count, created_at, updated_at)
        VALUES
          ('fixture-rebind', '{OWNER_ID}', '{GROUP_DID}',
           'peer.fixture.invalid', 'did:wba:old-peer.fixture.invalid',
           '{PEER_DID}', '2', 'awaiting_p6', '{{"fixture":true}}', 0,
           '{FIXED_TIME}', '{FIXED_TIME}');

        COMMIT;
        """
    )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--daemon-binary", type=Path, required=True)
    parser.add_argument("--source-ref", required=True)
    parser.add_argument("--artifact-version", required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    source_ref = args.source_ref.lower()
    if source_ref != SOURCE_REF or not re.fullmatch(r"[0-9a-f]{40}", source_ref):
        raise SystemExit(f"--source-ref must be the locked 0714 ref {SOURCE_REF}")
    if args.artifact_version != ARTIFACT_VERSION:
        raise SystemExit(f"--artifact-version must be {ARTIFACT_VERSION}")
    daemon_binary = args.daemon_binary.resolve()
    if not daemon_binary.is_file():
        raise SystemExit("--daemon-binary must point to an existing file")

    output_dir = args.output_dir.resolve()
    output_dir.mkdir(parents=True, exist_ok=True)
    fixture_path = output_dir / "local-state.sqlite"
    manifest_path = output_dir / "manifest.json"
    readme_path = output_dir / "README.md"
    if fixture_path.exists() or manifest_path.exists() or readme_path.exists():
        raise SystemExit("refusing to replace an existing locked fixture")

    with tempfile.TemporaryDirectory(prefix="awiki-release-0714-core-fixture-") as root:
        state_root = Path(root) / "state"
        completed = subprocess.run(
            [str(daemon_binary), "init-state", "--state-root", str(state_root)],
            check=True,
            capture_output=True,
            text=True,
        )
        report = json.loads(completed.stdout)
        if report["im_core_schema_version"] != SOURCE_SCHEMA_VERSION:
            raise SystemExit("historical daemon did not initialize schema 36")

        source_path = Path(report["im_core_sqlite_path"])
        connection = sqlite3.connect(source_path)
        try:
            assert_empty_user_tables(connection)
            insert_synthetic_rows(connection)
            if connection.execute("PRAGMA integrity_check").fetchone()[0] != "ok":
                raise SystemExit("generated fixture failed integrity_check")
            if connection.execute("PRAGMA foreign_key_check").fetchall():
                raise SystemExit("generated fixture failed foreign_key_check")
            connection.execute("PRAGMA wal_checkpoint(TRUNCATE)")
            connection.execute("PRAGMA journal_mode=DELETE")
            connection.execute("VACUUM")
            source_schema_fingerprint = schema_fingerprint(connection)
            snapshot_digest = conservation_digest(connection)
            oracles = {
                "messages": rows(connection, "SELECT COUNT(*) FROM messages")[0][0],
                "unread": rows(
                    connection,
                    "SELECT SUM(unread_count) FROM conversation_summaries",
                )[0][0],
                "unreadMentions": rows(
                    connection,
                    "SELECT SUM(unread_mention_count) FROM conversation_summaries",
                )[0][0],
                "awaitingP6": rows(
                    connection,
                    "SELECT COUNT(*) FROM group_rebind_outbox WHERE phase='awaiting_p6'",
                )[0][0],
            }
        finally:
            connection.close()
        shutil.copyfile(source_path, fixture_path)

    manifest = {
        "formatVersion": 1,
        "fixtureId": "release-0714-core-schema-36",
        "synthetic": True,
        "networkAccess": False,
        "identityDomainSuffix": ".fixture.invalid",
        "generator": {
            "path": "scripts/generate_release_0714_core_fixture.py",
            "sha256": sha256_file(Path(__file__).resolve()),
        },
        "sourceArtifact": {
            "component": "awiki-deamon",
            "version": args.artifact_version,
            "sourceRef": source_ref,
            "sha256": sha256_file(daemon_binary),
            "build": "local locked-source build",
        },
        "sourceDependencies": {
            "anp": {
                "sourceRef": ANP_SOURCE_REF,
                "version": "0.9.3",
            },
        },
        "sourceSchema": {
            "version": SOURCE_SCHEMA_VERSION,
            "fingerprint": source_schema_fingerprint,
        },
        "fixture": {
            "file": fixture_path.name,
            "sha256": sha256_file(fixture_path),
            "containsOnlySyntheticData": True,
        },
        "oracles": {
            **oracles,
            "conservationSha256": snapshot_digest,
        },
    }
    manifest_path.write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(json.dumps(manifest, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
