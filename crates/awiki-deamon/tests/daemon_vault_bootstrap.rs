use awiki_deamon::agent::{AgentIdentityRecord, AgentKind};
use awiki_deamon::{DaemonConfig, DaemonState};
use std::sync::{Mutex, MutexGuard};

static ENV_LOCK: Mutex<()> = Mutex::new(());
const DAEMON_VAULT_ROOT_KEY_ENV: &str = "AWIKI_DAEMON_VAULT_ROOT_KEY_B64";

#[test]
fn daemon_state_open_creates_local_root_key_and_encrypts_agent_identity() {
    let _env = EnvGuard::without_daemon_root_key();
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let root_key_file = local_root_key_file(&config);

    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();

    assert!(root_key_file.is_file());
    assert_eq!(
        root_key_file.file_name().and_then(|name| name.to_str()),
        Some("root-key.b64u")
    );
    assert_private_file_mode(&root_key_file);
    assert_private_dir_mode(root_key_file.parent().unwrap());

    let identity = identity_fixture(
        "did:wba:awiki.info:agent:daemon:vault-test",
        "edgehost-vault-test",
    );
    let auth_private_key = identity.auth_private_key_pem.clone();
    let signing_private_key = identity.e2ee_signing_private_key_pem.clone();
    let agreement_private_key = identity.e2ee_agreement_private_key_pem.clone();

    state.store_agent_identity(&identity).unwrap();
    let loaded = state.load_agent_identity(&identity.agent_did).unwrap();

    assert_eq!(loaded.auth_private_key_pem, auth_private_key);
    assert_eq!(loaded.e2ee_signing_private_key_pem, signing_private_key);
    assert_eq!(loaded.e2ee_agreement_private_key_pem, agreement_private_key);
    assert_agent_identity_row_uses_vault_refs(&state, &identity.agent_did);
    assert_no_plaintext_secret(&config.daemon_db_path, &auth_private_key);
    assert_no_plaintext_secret(&config.daemon_db_path, &signing_private_key);
    assert_no_plaintext_secret(&config.daemon_db_path, &agreement_private_key);
    assert_vault_records_do_not_contain_plaintext(&config, &auth_private_key);
    assert_vault_records_do_not_contain_plaintext(&config, &signing_private_key);
    assert_vault_records_do_not_contain_plaintext(&config, &agreement_private_key);
}

#[test]
fn daemon_state_reopens_local_root_key_for_identity_and_token_secrets() {
    let _env = EnvGuard::without_daemon_root_key();
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let first = DaemonState::open(&config).unwrap();
    first.initialize().unwrap();

    let identity = identity_fixture(
        "did:wba:awiki.info:agent:daemon:reopen-test",
        "edgehost-reopen-test",
    );
    let auth_private_key = identity.auth_private_key_pem.clone();
    first.store_agent_identity(&identity).unwrap();
    first
        .store_agent_auth_token(&identity.agent_did, "jwt-secret-for-reopen")
        .unwrap();
    drop(first);

    let reopened = DaemonState::open(&config).unwrap();
    reopened.initialize().unwrap();

    assert_eq!(
        reopened
            .load_agent_identity(&identity.agent_did)
            .unwrap()
            .auth_private_key_pem,
        auth_private_key
    );
    assert_eq!(
        reopened
            .load_agent_auth_token(&identity.agent_did)
            .unwrap()
            .as_deref(),
        Some("jwt-secret-for-reopen")
    );
    assert_no_plaintext_secret(&config.daemon_db_path, "jwt-secret-for-reopen");
    assert_vault_records_do_not_contain_plaintext(&config, "jwt-secret-for-reopen");
}

#[cfg(unix)]
#[test]
fn daemon_state_open_rejects_local_root_key_symlink() {
    let _env = EnvGuard::without_daemon_root_key();
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    std::fs::create_dir_all(&config.secret_vault_dir).unwrap();
    let symlink = local_root_key_file(&config);
    std::os::unix::fs::symlink(root.path().join("outside-root-key.b64u"), &symlink).unwrap();

    let error = DaemonState::open(&config).unwrap_err();
    let error_chain = format!("{error:?}");

    assert!(error_chain.contains("symlink"));
}

struct EnvGuard {
    _lock: MutexGuard<'static, ()>,
    previous_root_key: Option<String>,
}

impl EnvGuard {
    fn without_daemon_root_key() -> Self {
        let lock = ENV_LOCK.lock().unwrap();
        let previous_root_key = std::env::var(DAEMON_VAULT_ROOT_KEY_ENV).ok();
        std::env::remove_var(DAEMON_VAULT_ROOT_KEY_ENV);
        Self {
            _lock: lock,
            previous_root_key,
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(value) = self.previous_root_key.as_deref() {
            std::env::set_var(DAEMON_VAULT_ROOT_KEY_ENV, value);
        } else {
            std::env::remove_var(DAEMON_VAULT_ROOT_KEY_ENV);
        }
    }
}

fn local_root_key_file(config: &DaemonConfig) -> std::path::PathBuf {
    config.secret_vault_dir.join("root-key.b64u")
}

fn identity_fixture(agent_did: &str, handle: &str) -> AgentIdentityRecord {
    AgentIdentityRecord {
        agent_did: agent_did.to_string(),
        handle: handle.to_string(),
        agent_kind: AgentKind::Daemon,
        did_document: serde_json::json!({ "id": agent_did }),
        endpoint_url: Some("https://awiki.info/anp-im/rpc".to_string()),
        key_algorithm: "JsonWebKey2020".to_string(),
        public_key: "public-key".to_string(),
        auth_private_key_pem:
            "-----BEGIN PRIVATE KEY-----\nauth-private-secret\n-----END PRIVATE KEY-----"
                .to_string(),
        e2ee_signing_private_key_pem:
            "-----BEGIN PRIVATE KEY-----\nsigning-private-secret\n-----END PRIVATE KEY-----"
                .to_string(),
        e2ee_agreement_private_key_pem:
            "-----BEGIN PRIVATE KEY-----\nagreement-private-secret\n-----END PRIVATE KEY-----"
                .to_string(),
    }
}

fn assert_agent_identity_row_uses_vault_refs(state: &DaemonState, agent_did: &str) {
    let connection = state.connection().unwrap();
    let row: (
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
    ) = connection
        .query_row(
            r#"
SELECT
    auth_private_key_pem,
    e2ee_signing_private_key_pem,
    e2ee_agreement_private_key_pem,
    auth_private_key_ref_json,
    e2ee_signing_private_key_ref_json,
    e2ee_agreement_private_key_ref_json
FROM agent_identity
WHERE agent_did = ?1
"#,
            [agent_did],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(row.0, "<awiki-secret-vault-ref>");
    assert_eq!(row.1, "<awiki-secret-vault-ref>");
    assert_eq!(row.2, "<awiki-secret-vault-ref>");
    assert!(row.3.is_some());
    assert!(row.4.is_some());
    assert!(row.5.is_some());
}

fn assert_no_plaintext_secret(path: &std::path::Path, secret: &str) {
    if secret.is_empty() {
        return;
    }
    let raw = std::fs::read(path).unwrap();
    let text = String::from_utf8_lossy(&raw);
    assert!(
        !text.contains(secret),
        "{} leaked plaintext secret",
        path.display()
    );
}

fn assert_vault_records_do_not_contain_plaintext(config: &DaemonConfig, secret: &str) {
    if secret.is_empty() {
        return;
    }
    let records_dir = config.secret_vault_dir.join("records");
    let mut checked = 0;
    for entry in std::fs::read_dir(&records_dir).unwrap() {
        let entry = entry.unwrap();
        if entry.path().extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        checked += 1;
        let raw = std::fs::read(entry.path()).unwrap();
        let text = String::from_utf8_lossy(&raw);
        assert!(
            !text.contains(secret),
            "vault record leaked plaintext secret"
        );
    }
    assert!(checked > 0, "expected at least one vault record");
}

#[cfg(unix)]
fn assert_private_file_mode(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "{} mode should be 0600", path.display());
}

#[cfg(not(unix))]
fn assert_private_file_mode(_path: &std::path::Path) {}

#[cfg(unix)]
fn assert_private_dir_mode(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700, "{} mode should be 0700", path.display());
}

#[cfg(not(unix))]
fn assert_private_dir_mode(_path: &std::path::Path) {}
