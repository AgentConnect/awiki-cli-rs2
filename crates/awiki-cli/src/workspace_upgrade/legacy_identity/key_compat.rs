use super::layout::{write_secure_text, Manager};
use super::types::{IdentityError, Paths, INDEX_FILE_NAME};
use anp::PrivateKeyMaterial;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

const ANP_SECP256K1_PRIVATE_KEY_LABEL: &str = "ANP SECP256K1 PRIVATE KEY";
const ANP_SECP256R1_PRIVATE_KEY_LABEL: &str = "ANP SECP256R1 PRIVATE KEY";
const ANP_ED25519_PRIVATE_KEY_LABEL: &str = "ANP ED25519 PRIVATE KEY";
const ANP_X25519_PRIVATE_KEY_LABEL: &str = "ANP X25519 PRIVATE KEY";

struct PemBlock {
    label: String,
}

pub(crate) fn ensure_identity_private_keys_compatible(paths: &Paths) -> Result<(), IdentityError> {
    for (path, name) in [
        (&paths.key1_private_path, "key-1 private key"),
        (&paths.e2ee_signing_private_path, "e2ee signing private key"),
        (
            &paths.e2ee_agreement_private_path,
            "e2ee agreement private key",
        ),
    ] {
        ensure_private_key_pem_compatible(path, name)?;
    }
    Ok(())
}

pub(crate) fn ensure_all_identity_private_keys_compatible(
    workspace_paths: &crate::workspace_config::Paths,
) -> Result<(), IdentityError> {
    let identity_root = Path::new(&workspace_paths.identity_dir);
    let raw = match fs::read(identity_root.join(INDEX_FILE_NAME)) {
        Ok(raw) => raw,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(IdentityError::Io(err)),
    };
    for dir_name in indexed_identity_dir_names(&raw)? {
        ensure_identity_dir_private_keys_compatible(workspace_paths, &dir_name)?;
    }
    Ok(())
}

fn indexed_identity_dir_names(raw: &[u8]) -> Result<Vec<String>, IdentityError> {
    if let Ok(file) = serde_json::from_slice::<SdkRegistryFile>(raw) {
        if file.default_identity.is_some() || !file.identities.is_empty() {
            return Ok(file
                .identities
                .into_iter()
                .map(|entry| sdk_identity_dir_name(&entry))
                .filter(|dir_name| !dir_name.trim().is_empty())
                .collect());
        }
    }
    let file: LegacyRegistryFile = serde_json::from_slice(raw)?;
    Ok(file
        .credentials
        .into_iter()
        .map(|(alias, entry)| legacy_identity_dir_name(&alias, &entry))
        .filter(|dir_name| !dir_name.trim().is_empty())
        .collect())
}

fn ensure_private_key_pem_compatible(path: &str, name: &str) -> Result<(), IdentityError> {
    let raw = match fs::read(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(IdentityError::Io(err)),
    };
    let Some(normalized) = normalize_private_key_pem_to_pkcs8(&raw, name)? else {
        return Ok(());
    };
    write_secure_text(path, &normalized).map_err(|err| {
        IdentityError::Internal(format!("rewrite {name} as standard PKCS#8 PEM: {err}"))
    })?;
    Ok(())
}

fn ensure_identity_dir_private_keys_compatible(
    workspace_paths: &crate::workspace_config::Paths,
    dir_name: &str,
) -> Result<(), IdentityError> {
    let manager = Manager::new(workspace_paths.clone());
    let paths = manager.build_paths(dir_name);
    ensure_identity_private_keys_compatible(&paths)
}

fn normalize_private_key_pem_to_pkcs8(
    raw: &[u8],
    name: &str,
) -> Result<Option<String>, IdentityError> {
    let trimmed = trim_ascii_whitespace(raw);
    if trimmed.is_empty() {
        return Err(auth_required(format!("{name} is empty")));
    }
    let text = std::str::from_utf8(trimmed)
        .map_err(|_| auth_required(format!("invalid {name} PEM structure")))?;
    let block = decode_single_pem(text, name)?;

    let private_key = match block.label.as_str() {
        "PRIVATE KEY" => PrivateKeyMaterial::from_compatible_private_pem(text)
            .map_err(|err| auth_required(format!("unsupported {name} format: {err}")))?,
        "EC PRIVATE KEY" => {
            PrivateKeyMaterial::from_compatible_private_pem(text).map_err(|err| {
                auth_required(format!(
                    "unsupported {name} format ({}): {err}",
                    block.label
                ))
            })?
        }
        ANP_ED25519_PRIVATE_KEY_LABEL
        | ANP_X25519_PRIVATE_KEY_LABEL
        | ANP_SECP256R1_PRIVATE_KEY_LABEL
        | ANP_SECP256K1_PRIVATE_KEY_LABEL => PrivateKeyMaterial::from_compatible_private_pem(text)
            .map_err(|err| {
                auth_required(format!(
                    "unsupported {name} format ({}): {err}",
                    block.label
                ))
            })?,
        label => {
            return Err(auth_required(format!(
                "unsupported {name} PEM label {label:?}"
            )))
        }
    };

    let normalized = private_key.to_pem();
    if normalized.trim().is_empty() {
        return Err(IdentityError::Internal(
            "private key cannot be encoded as standard PKCS#8 PEM".to_string(),
        ));
    }
    if trim_trailing_lf(raw) == trim_trailing_lf(normalized.as_bytes()) {
        return Ok(None);
    }
    Ok(Some(normalized))
}

fn sdk_identity_dir_name(entry: &SdkIdentityRecord) -> String {
    first_non_empty([
        entry.dir_name.as_deref().unwrap_or_default(),
        entry.local_alias.as_deref().unwrap_or_default(),
        &entry.id,
    ])
    .unwrap_or_default()
    .to_string()
}

fn legacy_identity_dir_name(alias: &str, entry: &LegacyIdentityRecord) -> String {
    first_non_empty([
        &entry.dir_name,
        &entry.unique_id,
        &entry.credential_name,
        alias,
    ])
    .unwrap_or(alias)
    .to_string()
}

fn first_non_empty<'a>(values: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    values
        .into_iter()
        .map(str::trim)
        .find(|value| !value.is_empty())
}

fn decode_single_pem(input: &str, name: &str) -> Result<PemBlock, IdentityError> {
    let mut lines = input.lines();
    let begin = lines
        .next()
        .ok_or_else(|| auth_required(format!("invalid {name} PEM structure")))?;
    if !begin.starts_with("-----BEGIN ") || !begin.ends_with("-----") {
        return Err(auth_required(format!("invalid {name} PEM structure")));
    }
    let label = begin
        .trim_start_matches("-----BEGIN ")
        .trim_end_matches("-----")
        .to_string();
    if label.is_empty() {
        return Err(auth_required(format!("invalid {name} PEM structure")));
    }
    let end_marker = format!("-----END {label}-----");
    let mut body = String::new();
    let mut found_end = false;
    let mut reading_headers = true;
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed == end_marker {
            found_end = true;
            break;
        }
        if trimmed.is_empty() {
            reading_headers = false;
            continue;
        }
        if reading_headers && trimmed.contains(':') {
            continue;
        }
        reading_headers = false;
        body.push_str(trimmed);
    }
    if !found_end || body.is_empty() {
        return Err(auth_required(format!("invalid {name} PEM structure")));
    }
    if lines.any(|line| !line.trim().is_empty()) {
        return Err(auth_required(format!("invalid {name} PEM structure")));
    }
    STANDARD
        .decode(body.as_bytes())
        .map_err(|_| auth_required(format!("invalid {name} PEM structure")))?;
    Ok(PemBlock { label })
}

fn auth_required(message: String) -> IdentityError {
    IdentityError::AuthRequired(format!("authentication required: {message}"))
}

fn trim_ascii_whitespace(bytes: &[u8]) -> &[u8] {
    let mut start = 0;
    let mut end = bytes.len();
    while start < end && bytes[start].is_ascii_whitespace() {
        start += 1;
    }
    while end > start && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    &bytes[start..end]
}

fn trim_trailing_lf(bytes: &[u8]) -> &[u8] {
    let mut end = bytes.len();
    while end > 0 && bytes[end - 1] == b'\n' {
        end -= 1;
    }
    &bytes[..end]
}

#[derive(Debug, Deserialize)]
struct SdkRegistryFile {
    #[serde(default)]
    default_identity: Option<String>,
    #[serde(default)]
    identities: Vec<SdkIdentityRecord>,
}

#[derive(Debug, Deserialize)]
struct SdkIdentityRecord {
    id: String,
    #[serde(default)]
    dir_name: Option<String>,
    #[serde(default)]
    local_alias: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LegacyRegistryFile {
    #[serde(default)]
    credentials: BTreeMap<String, LegacyIdentityRecord>,
}

#[derive(Debug, Deserialize)]
struct LegacyIdentityRecord {
    #[serde(default)]
    credential_name: String,
    #[serde(default)]
    dir_name: String,
    #[serde(default)]
    unique_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn load_migrates_legacy_anp_private_keys_to_pkcs8_like_go() {
        let workspace = TempDir::new("key-compat-ed-r1-x").expect("workspace");
        let manager = identity_manager(workspace.path());
        let record = manager
            .save(
                crate::workspace_upgrade::legacy_identity::types::SaveInput {
                    identity_name: "default".to_string(),
                    did: "did:wba:awiki.ai:user:e1_keycompat".to_string(),
                    unique_id: "e1_keycompat".to_string(),
                    display_name: "Alice".to_string(),
                    did_document: Some(serde_json::json!({
                        "id": "did:wba:awiki.ai:user:e1_keycompat"
                    })),
                    key1_public_pem: "public".to_string(),
                    ..Default::default()
                },
            )
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
            assert_standard_private_key(
                path,
                &std::fs::read_to_string(path).expect("read key file"),
            );
        }
    }

    #[test]
    fn load_migrates_legacy_anp_secp256k1_key1_like_go() {
        let workspace = TempDir::new("key-compat-k1").expect("workspace");
        let manager = identity_manager(workspace.path());
        let record = manager
            .save(
                crate::workspace_upgrade::legacy_identity::types::SaveInput {
                    identity_name: "alice".to_string(),
                    did: "did:wba:awiki.ai:user:k1_keycompat".to_string(),
                    unique_id: "k1_keycompat".to_string(),
                    key1_public_pem: "public".to_string(),
                    ..Default::default()
                },
            )
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
    fn load_reports_go_shaped_auth_required_for_bad_private_key_pem() {
        let workspace = TempDir::new("key-compat-bad").expect("workspace");
        let manager = identity_manager(workspace.path());
        let record = manager
            .save(
                crate::workspace_upgrade::legacy_identity::types::SaveInput {
                    identity_name: "bad".to_string(),
                    did: "did:wba:awiki.ai:user:e1_bad".to_string(),
                    unique_id: "e1_bad".to_string(),
                    ..Default::default()
                },
            )
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
        anp::PrivateKeyMaterial::from_pem(value).expect("migrated private key parses");
    }

    fn assert_file_contains(path: &str, needle: &str) {
        let value = std::fs::read_to_string(path).expect("read file");
        assert!(
            value.contains(needle),
            "{path} should contain {needle:?}, got {value:?}"
        );
    }

    fn identity_manager(workspace: &Path) -> crate::workspace_upgrade::legacy_identity::Manager {
        crate::workspace_upgrade::legacy_identity::Manager::new(crate::workspace_config::Paths {
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
                "awiki-cli-rs2-{prefix}-{}-{nanos}",
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
}
