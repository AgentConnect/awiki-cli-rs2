#![cfg(feature = "sqlite")]

use rusqlite::{Connection, OpenFlags};
#[cfg(feature = "group-e2ee")]
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs;

const LOCKED_FIXTURE_SHA256: &str =
    "3f1b1ad19e9f7057bb98f413811038cb99205343c0d054950dcc0e1e2acbe4e0";
const LOCKED_GENERATOR_PATH: &str = "scripts/generate_release_0714_core_fixture.py";
const LOCKED_GENERATOR_SHA256: &str =
    "efb01c57d5e371d315b063bddc79ba29d6eaa8ab47693af663df559b7a3d8e8b";
const LOCKED_CONSERVATION_SHA256: &str =
    "7d7ca5a4de338ee85627b5bf07e54bd5fcf688ecc28c5ddef3626d42c609791c";
const LOCKED_SOURCE_REF: &str = "e2cf7f4cd00debba5353980e6d33c3ba682cdd0c";
const LOCKED_SOURCE_ARTIFACT_SHA256: &str =
    "12c92398f11b5b8223e02bf68f2f6afa8701b59b7fdeb9bbb38571fe3186c124";
const LOCKED_ANP_SOURCE_REF: &str = "59475cf76b23838a911a7263287ce6b7399d8e02";
const LOCKED_SOURCE_SCHEMA_FINGERPRINT: &str =
    "sha256:72822725c300c3aa03c436a667351c9d653abe2db7c78003289a1b410d3fd9aa";

#[cfg(feature = "group-e2ee")]
use anp::direct_e2ee::{
    DirectCipherBody, DirectE2eeSession, DirectEnvelopeMetadata, DirectInitBody, DirectSessionState,
};
#[cfg(feature = "group-e2ee")]
use anp::group_e2ee::operations::{decrypt, DecryptInput};
#[cfg(feature = "group-e2ee")]
use anp::group_e2ee::storage::CompatDataDirStore;
#[cfg(feature = "group-e2ee")]
use awiki_im_core::vault::{
    DeviceVaultRootKey, FileSecretVault, FileSecretVaultStore, SecretRef, SecretVault,
};
#[cfg(feature = "group-e2ee")]
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
#[cfg(feature = "group-e2ee")]
use x25519_dalek::StaticSecret as X25519StaticSecret;

const SNAPSHOT_QUERIES: &[(&str, usize)] = &[
    (
        "SELECT owner_identity_id,current_did,identity_generation,device_auth_generation \
         FROM identity_account_bindings ORDER BY owner_identity_id",
        4,
    ),
    (
        "SELECT conversation_id,thread_kind,thread_id,lifecycle_state,resolution_state \
         FROM conversation_registry ORDER BY conversation_id",
        5,
    ),
    (
        "SELECT conversation_id,CAST(message_count AS TEXT),CAST(unread_count AS TEXT),\
         CAST(unread_mention_count AS TEXT),COALESCE(first_unread_mention_message_id,'') \
         FROM conversation_summaries ORDER BY conversation_id",
        5,
    ),
    (
        "SELECT msg_id,conversation_id,CAST(server_seq AS TEXT),CAST(is_read AS TEXT),\
         CAST(mentions_current_user AS TEXT) FROM messages ORDER BY msg_id",
        5,
    ),
    (
        "SELECT conversation_id,peer_user_id,full_handle,current_did \
         FROM direct_peer_routes ORDER BY conversation_id",
        4,
    ),
    (
        "SELECT outbox_id,local_status,CAST(attempt_count AS TEXT),plaintext \
         FROM e2ee_outbox ORDER BY outbox_id",
        4,
    ),
    (
        "SELECT sync_subject_id,scope,checkpoint_kind,event_seq \
         FROM sync_state ORDER BY sync_subject_id,scope,checkpoint_kind",
        4,
    ),
    (
        "SELECT thread_kind,thread_id,message_id,content \
         FROM attachment_manifest_cache ORDER BY thread_kind,thread_id,message_id",
        4,
    ),
    (
        "SELECT job_id,phase,CAST(attempt_count AS TEXT),group_state_ref_json \
         FROM group_rebind_outbox ORDER BY job_id",
        4,
    ),
];

#[test]
fn locked_0714_schema_36_fixture_migrates_to_current_without_data_drift() {
    let fixture_dir = locked_core_fixture_dir();
    let manifest = read_fixture_manifest(&fixture_dir);
    validate_locked_fixture_manifest(&manifest, &fixture_dir).unwrap();

    let source = fixture_dir.join("local-state.sqlite");
    let temp = tempfile::tempdir().unwrap();
    let migrated = temp.path().join("core-schema-current.sqlite");
    fs::copy(source, &migrated).unwrap();

    let before = Connection::open_with_flags(&migrated, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert_eq!(
        awiki_im_core::compat::local_state::current_schema_version(&before).unwrap(),
        36
    );
    let before_digest = conservation_digest(&before);
    assert_eq!(before_digest, LOCKED_CONSERVATION_SHA256);
    assert_eq!(scalar(&before, "SELECT COUNT(*) FROM messages"), 2);
    assert_eq!(
        scalar(
            &before,
            "SELECT SUM(unread_count) FROM conversation_summaries"
        ),
        1
    );
    assert_eq!(
        scalar(
            &before,
            "SELECT SUM(unread_mention_count) FROM conversation_summaries"
        ),
        1
    );
    assert_eq!(
        scalar(
            &before,
            "SELECT COUNT(*) FROM group_rebind_outbox WHERE phase='awaiting_p6'"
        ),
        1
    );
    drop(before);

    let connection = Connection::open(&migrated).unwrap();
    awiki_im_core::compat::local_state::ensure_schema(&connection).unwrap();
    assert_eq!(
        awiki_im_core::compat::local_state::current_schema_version(&connection).unwrap(),
        awiki_im_core::compat::local_state::SCHEMA_VERSION
    );
    assert_eq!(conservation_digest(&connection), before_digest);
    assert_eq!(
        scalar(
            &connection,
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='message_sync_run_state'"
        ),
        1
    );
    assert_eq!(
        scalar(
            &connection,
            "SELECT COUNT(*) FROM pragma_table_info('sync_lane_capability_state') WHERE name IN ('client_instance_id','negotiated_capabilities_json')"
        ),
        2
    );
    assert_eq!(
        scalar(&connection, "SELECT COUNT(*) FROM did_transition_edges"),
        0
    );
    assert_eq!(
        scalar(
            &connection,
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND \
             (lower(name) LIKE '%conflict%' OR lower(name) LIKE '%reconcile%')",
        ),
        0
    );
    eprintln!(
        "phase2_0714_core_migrated_sha256={:x}",
        Sha256::digest(fs::read(&migrated).unwrap())
    );
}

#[test]
fn locked_0714_manifest_rejects_provenance_and_checksum_mutations() {
    let fixture_dir = locked_core_fixture_dir();
    let manifest = read_fixture_manifest(&fixture_dir);
    let mutation_cases = [
        ("/generator/path", serde_json::json!("scripts/other.py")),
        ("/generator/sha256", serde_json::json!("0".repeat(64))),
        ("/sourceArtifact/sha256", serde_json::json!("1".repeat(64))),
        (
            "/sourceDependencies/anp/sourceRef",
            serde_json::json!("2".repeat(40)),
        ),
        (
            "/sourceSchema/fingerprint",
            serde_json::json!(format!("sha256:{}", "3".repeat(64))),
        ),
        ("/fixture/sha256", serde_json::json!("4".repeat(64))),
    ];

    for (pointer, replacement) in mutation_cases {
        let mut mutated = manifest.clone();
        *mutated.pointer_mut(pointer).unwrap() = replacement;
        assert!(
            validate_locked_fixture_manifest(&mutated, &fixture_dir).is_err(),
            "manifest mutation must fail: {pointer}"
        );
    }
}

fn locked_core_fixture_dir() -> std::path::PathBuf {
    std::env::var_os("AWIKI_0714_CORE_FIXTURE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/release_0714_core")
        })
}

fn read_fixture_manifest(fixture_dir: &std::path::Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(fixture_dir.join("manifest.json")).unwrap()).unwrap()
}

fn validate_locked_fixture_manifest(
    manifest: &serde_json::Value,
    fixture_dir: &std::path::Path,
) -> Result<(), String> {
    macro_rules! require_eq {
        ($actual:expr, $expected:expr, $label:literal) => {
            if $actual != $expected {
                return Err($label.to_owned());
            }
        };
    }

    require_eq!(manifest["formatVersion"], 1, "format version");
    require_eq!(
        manifest["fixtureId"],
        "release-0714-core-schema-36",
        "fixture id"
    );
    require_eq!(manifest["synthetic"], true, "synthetic marker");
    require_eq!(manifest["networkAccess"], false, "offline marker");
    require_eq!(
        manifest["identityDomainSuffix"],
        ".fixture.invalid",
        "identity domain"
    );
    require_eq!(
        manifest["fixture"]["containsOnlySyntheticData"],
        true,
        "synthetic fixture marker"
    );
    require_eq!(
        manifest["fixture"]["file"],
        "local-state.sqlite",
        "fixture file"
    );
    require_eq!(
        manifest["fixture"]["sha256"],
        LOCKED_FIXTURE_SHA256,
        "fixture checksum"
    );
    require_eq!(
        manifest["generator"]["path"],
        LOCKED_GENERATOR_PATH,
        "generator path"
    );
    require_eq!(
        manifest["generator"]["sha256"],
        LOCKED_GENERATOR_SHA256,
        "generator checksum"
    );
    require_eq!(
        manifest["sourceArtifact"]["sourceRef"],
        LOCKED_SOURCE_REF,
        "source ref"
    );
    require_eq!(
        manifest["sourceArtifact"]["sha256"],
        LOCKED_SOURCE_ARTIFACT_SHA256,
        "source artifact checksum"
    );
    require_eq!(
        manifest["sourceDependencies"]["anp"]["sourceRef"],
        LOCKED_ANP_SOURCE_REF,
        "ANP source ref"
    );
    require_eq!(manifest["sourceSchema"]["version"], 36, "schema version");
    require_eq!(
        manifest["sourceSchema"]["fingerprint"],
        LOCKED_SOURCE_SCHEMA_FINGERPRINT,
        "schema fingerprint"
    );
    require_eq!(manifest["oracles"]["messages"], 2, "message count");
    require_eq!(manifest["oracles"]["unread"], 1, "unread count");
    require_eq!(
        manifest["oracles"]["unreadMentions"],
        1,
        "unread mention count"
    );
    require_eq!(manifest["oracles"]["awaitingP6"], 1, "P6 job count");
    require_eq!(
        manifest["oracles"]["conservationSha256"],
        LOCKED_CONSERVATION_SHA256,
        "conservation checksum"
    );

    let repository_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let generator = repository_root.join(LOCKED_GENERATOR_PATH);
    require_eq!(
        sha256_file(&generator),
        LOCKED_GENERATOR_SHA256,
        "computed generator checksum"
    );
    let fixture = fixture_dir.join("local-state.sqlite");
    require_eq!(
        sha256_file(&fixture),
        LOCKED_FIXTURE_SHA256,
        "computed fixture checksum"
    );
    let connection = Connection::open(&fixture).map_err(|_| "open fixture".to_owned())?;
    require_eq!(
        schema_fingerprint(&connection),
        LOCKED_SOURCE_SCHEMA_FINGERPRINT,
        "computed schema fingerprint"
    );
    Ok(())
}

fn sha256_file(path: &std::path::Path) -> String {
    format!("{:x}", Sha256::digest(fs::read(path).unwrap()))
}

fn schema_fingerprint(connection: &Connection) -> String {
    let mut statement = connection
        .prepare(
            "SELECT type, name, COALESCE(sql, '') FROM sqlite_schema \
             WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
        )
        .unwrap();
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .unwrap();
    let mut digest = Sha256::new();
    for row in rows {
        let (kind, name, sql) = row.unwrap();
        digest.update(kind.as_bytes());
        digest.update([0]);
        digest.update(name.as_bytes());
        digest.update([0]);
        for token in sql.split_whitespace() {
            digest.update(token.as_bytes());
            digest.update([b' ']);
        }
        digest.update([b'\n']);
    }
    format!("sha256:{:x}", digest.finalize())
}

#[cfg(feature = "group-e2ee")]
#[test]
fn phase3_candidate_decrypts_locked_0714_p5_and_p6_v1_ciphertext() {
    let fixture_dir = std::env::var_os("AWIKI_0714_E2EE_FIXTURE_DIR")
        .expect("AWIKI_0714_E2EE_FIXTURE_DIR must name the locked offline fixture");
    let temp = tempfile::tempdir().unwrap();
    let bundle = temp.path().join("fixture");
    copy_fixture_tree(std::path::Path::new(&fixture_dir), &bundle);

    let direct: DirectFixture = read_json(&bundle.join("direct-v1.json"));
    let root_key = decode_32(
        fs::read_to_string(bundle.join("vault-root-key.fixture.b64u"))
            .unwrap()
            .trim(),
    );
    let vault = FileSecretVault::new(
        DeviceVaultRootKey::from_bytes(root_key),
        FileSecretVaultStore::new(bundle.join("vault")),
    );
    let bob_static = X25519StaticSecret::from(open_32(&vault, &direct.bob_static_ref));
    let bob_signed_prekey =
        X25519StaticSecret::from(open_32(&vault, &direct.bob_signed_prekey_ref));
    let (_, init_plaintext) = DirectE2eeSession::accept_incoming_init(
        &direct.init_metadata,
        &direct.init_metadata.recipient_did,
        &bob_static,
        &bob_signed_prekey,
        &decode_32(&direct.alice_static_public_b64u),
        &direct.init_body,
    )
    .expect("current reader decrypts 0714 P5 v1 init");
    assert_eq!(
        digest_json(&init_plaintext),
        direct.expected_init_plaintext_sha256
    );

    let alice_state = vault.open(&direct.alice_session_ref).unwrap();
    let mut alice_session: DirectSessionState =
        serde_json::from_slice(alice_state.expose_secret()).unwrap();
    let cipher_plaintext = DirectE2eeSession::decrypt_follow_up(
        &mut alice_session,
        &direct.cipher_metadata,
        &direct.cipher_body,
        "application/json",
    )
    .expect("current reader decrypts 0714 P5 v1 cipher");
    assert_eq!(
        digest_json(&cipher_plaintext),
        direct.expected_cipher_plaintext_sha256
    );

    let group: GroupFixture = read_json(&bundle.join("group-v1.json"));
    let group_plaintext = decrypt(
        &CompatDataDirStore::new(bundle.join("group-mls/bob-reader")),
        DecryptInput {
            recipient_did: group.recipient_did,
            device_id: group.recipient_device_id,
            group_did: group.group_cipher_object["group_state_ref"]["group_did"]
                .as_str()
                .unwrap()
                .to_owned(),
            sender_did: group.sender_did,
            message_id: group.message_id,
            operation_id: group.operation_id,
            group_cipher_object: serde_json::from_value(group.group_cipher_object).unwrap(),
            request_id: "fixture-group-phase3-reader-0714".to_owned(),
        },
    )
    .expect("current reader decrypts 0714 P6 v1 cipher")
    .application_plaintext;
    assert_eq!(
        digest_json(&group_plaintext),
        group.expected_plaintext_sha256
    );
}

#[cfg(feature = "group-e2ee")]
#[derive(Deserialize)]
struct DirectFixture {
    init_metadata: DirectEnvelopeMetadata,
    init_body: DirectInitBody,
    cipher_metadata: DirectEnvelopeMetadata,
    cipher_body: DirectCipherBody,
    alice_session_ref: SecretRef,
    bob_static_ref: SecretRef,
    bob_signed_prekey_ref: SecretRef,
    alice_static_public_b64u: String,
    expected_init_plaintext_sha256: String,
    expected_cipher_plaintext_sha256: String,
}

#[cfg(feature = "group-e2ee")]
#[derive(Deserialize)]
struct GroupFixture {
    recipient_did: String,
    recipient_device_id: String,
    sender_did: String,
    message_id: String,
    operation_id: String,
    group_cipher_object: serde_json::Value,
    expected_plaintext_sha256: String,
}

#[cfg(feature = "group-e2ee")]
fn read_json<T: serde::de::DeserializeOwned>(path: &std::path::Path) -> T {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

#[cfg(feature = "group-e2ee")]
fn open_32(vault: &FileSecretVault, secret_ref: &SecretRef) -> [u8; 32] {
    vault
        .open(secret_ref)
        .unwrap()
        .expose_secret()
        .try_into()
        .unwrap()
}

#[cfg(feature = "group-e2ee")]
fn decode_32(value: &str) -> [u8; 32] {
    URL_SAFE_NO_PAD.decode(value).unwrap().try_into().unwrap()
}

#[cfg(feature = "group-e2ee")]
fn digest_json<T: serde::Serialize>(value: &T) -> String {
    format!("{:x}", Sha256::digest(serde_json::to_vec(value).unwrap()))
}

#[cfg(feature = "group-e2ee")]
fn copy_fixture_tree(source: &std::path::Path, destination: &std::path::Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_fixture_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn conservation_digest(connection: &Connection) -> String {
    let mut digest = Sha256::new();
    for (sql, columns) in SNAPSHOT_QUERIES {
        digest.update(sql.as_bytes());
        let mut statement = connection.prepare(sql).unwrap();
        let rows = statement
            .query_map([], |row| {
                (0..*columns)
                    .map(|index| row.get::<_, String>(index))
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap();
        for row in rows {
            for value in row.unwrap() {
                digest.update((value.len() as u64).to_be_bytes());
                digest.update(value.as_bytes());
            }
        }
    }
    format!("{:x}", digest.finalize())
}

fn scalar(connection: &Connection, sql: &str) -> i64 {
    connection.query_row(sql, [], |row| row.get(0)).unwrap()
}
