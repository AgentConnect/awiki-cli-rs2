use awiki_cli::anpsdk::PrivateKeyMaterial;
use awiki_cli::identity::types::SaveInput;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn manager_load_migrates_legacy_anp_private_keys_to_pkcs8_like_go() {
    let workspace = TempDir::new().expect("workspace");
    let manager = identity_manager(workspace.path());
    let record = manager
        .save(SaveInput {
            identity_name: "default".to_string(),
            did: "did:wba:awiki.ai:user:e1_keycompat".to_string(),
            unique_id: "e1_keycompat".to_string(),
            display_name: "Alice".to_string(),
            did_document: Some(serde_json::json!({
                "id": "did:wba:awiki.ai:user:e1_keycompat"
            })),
            key1_public_pem: "public".to_string(),
            ..Default::default()
        })
        .expect("save identity");
    let paths = manager.build_paths(&record.dir_name);
    std::fs::write(
        &paths.key1_private_path,
        legacy_private_pem("ANP ED25519 PRIVATE KEY", &[1; 32]),
    )
    .expect("write key-1 legacy pem");
    std::fs::write(
        &paths.e2ee_signing_private_path,
        legacy_private_pem("ANP SECP256R1 PRIVATE KEY", &[1]),
    )
    .expect("write signing legacy pem");
    std::fs::write(
        &paths.e2ee_agreement_private_path,
        legacy_private_pem("ANP X25519 PRIVATE KEY", &[2; 32]),
    )
    .expect("write agreement legacy pem");

    assert_file_contains(&paths.key1_private_path, "BEGIN ANP ED25519 PRIVATE KEY");
    assert_file_contains(
        &paths.e2ee_signing_private_path,
        "BEGIN ANP SECP256R1 PRIVATE KEY",
    );
    assert_file_contains(
        &paths.e2ee_agreement_private_path,
        "BEGIN ANP X25519 PRIVATE KEY",
    );

    let loaded = manager.load("default").expect("load identity");
    for (name, value) in [
        ("key-1", loaded.key1_private_pem.as_str()),
        ("e2ee signing", loaded.e2ee_signing_private_pem.as_str()),
        ("e2ee agreement", loaded.e2ee_agreement_private_pem.as_str()),
    ] {
        assert_standard_private_key(name, value);
    }
    for path in [
        &paths.key1_private_path,
        &paths.e2ee_signing_private_path,
        &paths.e2ee_agreement_private_path,
    ] {
        assert_standard_private_key(path, &std::fs::read_to_string(path).expect("read key file"));
    }
}

#[test]
fn manager_load_migrates_legacy_anp_secp256k1_key1_like_go() {
    let workspace = TempDir::new().expect("workspace");
    let manager = identity_manager(workspace.path());
    let record = manager
        .save(SaveInput {
            identity_name: "alice".to_string(),
            did: "did:wba:awiki.ai:user:k1_keycompat".to_string(),
            unique_id: "k1_keycompat".to_string(),
            key1_public_pem: "public".to_string(),
            ..Default::default()
        })
        .expect("save identity");
    let paths = manager.build_paths(&record.dir_name);
    std::fs::write(
        &paths.key1_private_path,
        legacy_private_pem("ANP SECP256K1 PRIVATE KEY", &[1]),
    )
    .expect("write key-1 legacy pem");

    let loaded = manager.load("alice").expect("load identity");
    assert_standard_private_key("key-1", &loaded.key1_private_pem);
    assert_standard_private_key(
        "key-1 file",
        &std::fs::read_to_string(&paths.key1_private_path).expect("read key file"),
    );
}

#[test]
fn manager_load_reports_go_shaped_auth_required_for_bad_private_key_pem() {
    let workspace = TempDir::new().expect("workspace");
    let manager = identity_manager(workspace.path());
    let record = manager
        .save(SaveInput {
            identity_name: "bad".to_string(),
            did: "did:wba:awiki.ai:user:e1_bad".to_string(),
            unique_id: "e1_bad".to_string(),
            ..Default::default()
        })
        .expect("save identity");
    let paths = manager.build_paths(&record.dir_name);
    std::fs::write(
        &paths.key1_private_path,
        "-----BEGIN UNKNOWN PRIVATE KEY-----\nAQ==\n-----END UNKNOWN PRIVATE KEY-----\n",
    )
    .expect("write bad private key");

    let err = manager.load("bad").expect_err("bad key should fail");
    assert!(
        err.to_string().contains(
            "authentication required: unsupported key-1 private key PEM label \"UNKNOWN PRIVATE KEY\""
        ),
        "unexpected error: {err}"
    );
    assert_file_contains(&paths.key1_private_path, "BEGIN UNKNOWN PRIVATE KEY");
}

fn legacy_private_pem(label: &str, contents: &[u8]) -> String {
    let encoded = STANDARD.encode(contents);
    let mut wrapped = String::new();
    for chunk in encoded.as_bytes().chunks(64) {
        wrapped.push_str(std::str::from_utf8(chunk).unwrap());
        wrapped.push('\n');
    }
    format!("-----BEGIN {label}-----\n{wrapped}-----END {label}-----\n")
}

fn assert_standard_private_key(name: &str, value: &str) {
    assert!(
        !value.contains("BEGIN ANP "),
        "{name} private key still uses legacy ANP PEM label"
    );
    assert!(
        value.starts_with("-----BEGIN PRIVATE KEY-----"),
        "{name} private key starts with {:?}",
        value.lines().next().unwrap_or_default()
    );
    PrivateKeyMaterial::from_pem(value).expect("migrated private key parses");
}

fn assert_file_contains(path: &str, needle: &str) {
    let value = std::fs::read_to_string(path).expect("read file");
    assert!(
        value.contains(needle),
        "{path} should contain {needle:?}, got {value:?}"
    );
}

fn identity_manager(workspace: &Path) -> awiki_cli::identity::Manager {
    awiki_cli::identity::Manager::new(awiki_cli::config::Paths {
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
    fn new() -> std::io::Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-key-compat-test-{}-{nanos}",
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
