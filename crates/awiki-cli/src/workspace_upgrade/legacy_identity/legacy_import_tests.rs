use super::{
    did::generate_identity,
    types::{GeneratedIdentity, IdentityError, IndexEntry, IndexPayload, SaveInput},
    Manager,
};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const LEGACY_E2EE_PREFIX: &str = "e2ee_";

#[test]
fn scan_legacy_detects_indexed_flat_invalid_and_orphan_artifacts_like_go() {
    let workspace = TempDir::new("scan-legacy").expect("workspace");
    let manager = identity_manager(workspace.path());
    let legacy_root = PathBuf::from(manager.legacy_root_dir());
    std::fs::create_dir_all(&legacy_root).expect("create legacy root");

    let indexed = write_legacy_indexed_credential(&legacy_root, "indexed", "legacy-indexed");
    write_legacy_index(&legacy_root, "indexed", [("indexed", indexed)]);
    write_legacy_flat_credential(&legacy_root, "flat");
    std::fs::write(legacy_root.join("broken.json"), "{").expect("write invalid json");
    std::fs::write(
        legacy_root.join("note.json"),
        r#"{"note":"not a credential"}"#,
    )
    .expect("write non-credential json");
    std::fs::write(
        legacy_root.join(format!("{LEGACY_E2EE_PREFIX}orphan.json")),
        r#"{"state":"orphan"}"#,
    )
    .expect("write orphan e2ee");

    let scan = manager.scan_legacy().expect("scan legacy");

    assert!(scan.has_legacy);
    assert!(scan.indexed_layout);
    assert_eq!(
        scan.indexed_entries
            .get("indexed")
            .expect("indexed entry")
            .dir_name,
        "legacy-indexed"
    );
    assert_eq!(scan.legacy_credentials.len(), 1);
    assert_eq!(scan.legacy_credentials[0].credential_name, "flat");

    let mut invalid_files = scan
        .invalid_json_files
        .iter()
        .map(|item| item["file"].as_str())
        .collect::<Vec<_>>();
    invalid_files.sort_unstable();
    assert_eq!(invalid_files, ["broken.json", "note.json"]);
    assert!(scan
        .invalid_json_files
        .iter()
        .any(|item| item["reason"].starts_with("invalid_json:")));
    assert!(scan
        .invalid_json_files
        .iter()
        .any(|item| item["reason"] == "not_a_legacy_credential_payload"));

    assert_eq!(scan.orphan_e2ee_files.len(), 1);
    assert_eq!(scan.orphan_e2ee_files[0]["credential_name"], "orphan");
    assert!(scan.orphan_e2ee_files[0]["file"].ends_with("e2ee_orphan.json"));
}

#[test]
fn import_legacy_requires_name_when_multiple_flat_credentials_exist_like_go() {
    let workspace = TempDir::new("import-requires-name").expect("workspace");
    let manager = identity_manager(workspace.path());
    let legacy_root = PathBuf::from(manager.legacy_root_dir());
    std::fs::create_dir_all(&legacy_root).expect("create legacy root");
    write_legacy_flat_credential(&legacy_root, "alice");
    write_legacy_flat_credential(&legacy_root, "bob");

    let err = manager
        .import_legacy(String::new())
        .expect_err("ambiguous import should fail");

    assert!(
        matches!(err, IdentityError::InvalidInput(_)),
        "unexpected error: {err}"
    );
    assert!(
        err.to_string()
            .contains("multiple legacy identities detected, specify --name or --all"),
        "unexpected error: {err}"
    );
}

#[test]
fn import_all_legacy_imports_indexed_default_and_copies_flat_e2ee_state_like_go() {
    let workspace = TempDir::new("import-all-default-e2ee").expect("workspace");
    let manager = identity_manager(workspace.path());
    let legacy_root = PathBuf::from(manager.legacy_root_dir());
    std::fs::create_dir_all(&legacy_root).expect("create legacy root");

    let indexed = write_legacy_indexed_credential(&legacy_root, "indexed", "legacy-indexed");
    write_legacy_index(&legacy_root, "indexed", [("indexed", indexed)]);
    write_legacy_flat_credential(&legacy_root, "flat");
    std::fs::write(
        legacy_root.join(format!("{LEGACY_E2EE_PREFIX}flat.json")),
        r#"{"state":"copied"}"#,
    )
    .expect("write flat e2ee state");

    let result = manager.import_all_legacy().expect("import all legacy");

    let imported_names = result
        .imported
        .iter()
        .map(|item| item.identity_name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(imported_names, ["indexed", "flat"]);
    assert!(result.skipped.is_empty());
    assert_eq!(
        manager.current().expect("current identity").identity_name,
        "indexed"
    );
    assert_eq!(
        std::fs::read_to_string(
            manager
                .paths_for_identity("flat")
                .expect("flat paths")
                .e2ee_state_path
        )
        .expect("read flat e2ee state"),
        r#"{"state":"copied"}"#
    );
}

#[test]
fn import_all_legacy_skips_conflicting_flat_credentials_like_go() {
    let workspace = TempDir::new("import-all-conflict").expect("workspace");
    let manager = identity_manager(workspace.path());
    let legacy_root = PathBuf::from(manager.legacy_root_dir());
    std::fs::create_dir_all(&legacy_root).expect("create legacy root");

    let conflict = write_legacy_flat_credential(&legacy_root, "conflict");
    manager
        .save(SaveInput {
            identity_name: "conflict".to_string(),
            did: "did:wba:legacy-import.example:user:e1_other".to_string(),
            unique_id: "e1_other".to_string(),
            display_name: "Conflict".to_string(),
            did_document: Some(conflict.did_document),
            key1_private_pem: conflict.key1_private_pem,
            key1_public_pem: conflict.key1_public_pem,
            ..SaveInput::default()
        })
        .expect("save existing conflicting identity");
    write_legacy_flat_credential(&legacy_root, "ok");

    let result = manager.import_all_legacy().expect("import all legacy");

    let imported_names = result
        .imported
        .iter()
        .map(|item| item.identity_name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(imported_names, ["ok"]);
    assert_eq!(result.skipped, ["conflict"]);
}

fn write_legacy_flat_credential(legacy_root: &Path, credential_name: &str) -> GeneratedIdentity {
    let generated =
        generate_identity("legacy-import.example", "", "").expect("generated legacy flat identity");
    write_json(
        &legacy_root.join(format!("{credential_name}.json")),
        &json!({
            "did": generated.did,
            "unique_id": generated.unique_id,
            "name": format!("Legacy {credential_name}"),
            "handle": credential_name,
            "jwt_token": format!("legacy-token-{credential_name}"),
            "private_key_pem": generated.key1_private_pem,
            "public_key_pem": generated.key1_public_pem,
            "e2ee_signing_private_pem": generated.e2ee_signing_private_pem,
            "e2ee_agreement_private_pem": generated.e2ee_agreement_private_pem,
            "did_document": generated.did_document,
        }),
    );
    generated
}

fn write_legacy_indexed_credential(
    legacy_root: &Path,
    credential_name: &str,
    dir_name: &str,
) -> IndexEntry {
    let generated = generate_identity("legacy-import.example", "", "")
        .expect("generated legacy indexed identity");
    let identity_dir = legacy_root.join(dir_name);
    std::fs::create_dir_all(&identity_dir).expect("create indexed identity dir");
    write_json(
        &identity_dir.join("identity.json"),
        &json!({
            "did": generated.did,
            "unique_id": generated.unique_id,
            "created_at": "2026-01-01T00:00:00Z",
            "name": format!("Indexed {credential_name}"),
            "handle": credential_name,
        }),
    );
    write_json(
        &identity_dir.join("auth.json"),
        &json!({ "jwt_token": format!("jwt-{credential_name}") }),
    );
    write_json(
        &identity_dir.join("did_document.json"),
        &generated.did_document,
    );
    std::fs::write(
        identity_dir.join("key-1-private.pem"),
        &generated.key1_private_pem,
    )
    .expect("write key-1 private");
    std::fs::write(
        identity_dir.join("key-1-public.pem"),
        &generated.key1_public_pem,
    )
    .expect("write key-1 public");
    std::fs::write(
        identity_dir.join("e2ee-signing-private.pem"),
        &generated.e2ee_signing_private_pem,
    )
    .expect("write e2ee signing private");
    std::fs::write(
        identity_dir.join("e2ee-agreement-private.pem"),
        &generated.e2ee_agreement_private_pem,
    )
    .expect("write e2ee agreement private");

    IndexEntry {
        credential_name: credential_name.to_string(),
        dir_name: dir_name.to_string(),
        did: generated.did,
        unique_id: generated.unique_id,
        user_id: format!("user-{credential_name}"),
        name: format!("Indexed {credential_name}"),
        handle: credential_name.to_string(),
        created_at: "2026-01-01T00:00:00Z".to_string(),
        ..IndexEntry::default()
    }
}

fn write_legacy_index<'a>(
    legacy_root: &Path,
    default_credential_name: &str,
    entries: impl IntoIterator<Item = (&'a str, IndexEntry)>,
) {
    write_json(
        &legacy_root.join("index.json"),
        &IndexPayload {
            default_credential_name: default_credential_name.to_string(),
            credentials: entries
                .into_iter()
                .map(|(name, entry)| (name.to_string(), entry))
                .collect::<BTreeMap<_, _>>(),
            ..IndexPayload::default()
        },
    );
}

fn write_json(path: &Path, value: &impl serde::Serialize) {
    std::fs::write(
        path,
        serde_json::to_vec_pretty(value).expect("serialize JSON fixture"),
    )
    .expect("write JSON fixture");
}

fn identity_manager(workspace: &Path) -> Manager {
    Manager::new(crate::workspace_config::Paths {
        workspace_home_dir: path_string(workspace),
        root_dir: path_string(workspace),
        config_dir: path_string(&workspace.join("config")),
        data_dir: path_string(&workspace.join("data")),
        state_dir: path_string(&workspace.join("state")),
        cache_dir: path_string(&workspace.join("cache")),
        logs_dir: path_string(&workspace.join("logs")),
        config_file: path_string(&workspace.join("config").join("config.yaml")),
        identity_dir: path_string(&workspace.join("identities")),
        database_file: path_string(&workspace.join("data").join("awiki.db")),
        legacy_credentials_dir: path_string(&workspace.join("legacy")),
        legacy_data_dir: path_string(&workspace.join("legacy-data")),
    })
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> std::io::Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-legacy-import-{prefix}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
