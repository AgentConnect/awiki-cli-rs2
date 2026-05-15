use super::types::{
    IdentityError, IndexEntry, IndexPayload, Paths, AUTH_FILE_NAME, DID_DOCUMENT_FILE_NAME,
    E2EE_AGREEMENT_PRIVATE_FILE_NAME, E2EE_SIGNING_PRIVATE_FILE_NAME, E2EE_STATE_FILE_NAME,
    IDENTITY_FILE_NAME, INDEX_FILE_NAME, INDEX_SCHEMA_VERSION, KEY1_PRIVATE_FILE_NAME,
    KEY1_PUBLIC_FILE_NAME,
};
use crate::config;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Manager {
    paths: config::Paths,
}

impl Manager {
    pub fn new(paths: config::Paths) -> Self {
        Self { paths }
    }

    pub fn root_dir(&self) -> &str {
        &self.paths.identity_dir
    }

    pub fn legacy_root_dir(&self) -> &str {
        &self.paths.legacy_credentials_dir
    }

    pub fn ensure_root(&self) -> Result<(), IdentityError> {
        ensure_dir(Path::new(self.root_dir()))
    }

    pub fn build_paths(&self, dir_name: &str) -> Paths {
        let root = Path::new(self.root_dir());
        let identity_dir = root.join(dir_name);
        Paths {
            root_dir: path_string(root),
            dir_name: dir_name.to_string(),
            identity_dir: path_string(&identity_dir),
            identity_path: path_string(&identity_dir.join(IDENTITY_FILE_NAME)),
            auth_path: path_string(&identity_dir.join(AUTH_FILE_NAME)),
            did_document_path: path_string(&identity_dir.join(DID_DOCUMENT_FILE_NAME)),
            key1_private_path: path_string(&identity_dir.join(KEY1_PRIVATE_FILE_NAME)),
            key1_public_path: path_string(&identity_dir.join(KEY1_PUBLIC_FILE_NAME)),
            e2ee_signing_private_path: path_string(
                &identity_dir.join(E2EE_SIGNING_PRIVATE_FILE_NAME),
            ),
            e2ee_agreement_private_path: path_string(
                &identity_dir.join(E2EE_AGREEMENT_PRIVATE_FILE_NAME),
            ),
            e2ee_state_path: path_string(&identity_dir.join(E2EE_STATE_FILE_NAME)),
        }
    }

    pub fn paths_for_identity(&self, name: &str) -> Result<Paths, IdentityError> {
        let index = self.load_index()?;
        let (_, entry) = self
            .resolve_entry_name(name, &index)
            .ok_or_else(|| IdentityError::NotFound(format!("identity not found: {name}")))?;
        Ok(self.build_paths(&entry.dir_name))
    }

    pub fn load_index(&self) -> Result<IndexPayload, IdentityError> {
        load_index_from(&Path::new(self.root_dir()).join(INDEX_FILE_NAME))
    }

    pub fn save_index(&self, payload: IndexPayload) -> Result<(), IdentityError> {
        self.ensure_root()?;
        save_index_to(&Path::new(self.root_dir()).join(INDEX_FILE_NAME), payload)
    }

    pub fn resolve_entry_name(
        &self,
        requested: &str,
        index: &IndexPayload,
    ) -> Option<(String, IndexEntry)> {
        if let Some(entry) = index.credentials.get(requested) {
            return Some((requested.to_string(), entry.clone()));
        }
        if requested.is_empty() || requested == "default" {
            if !index.default_credential_name.is_empty() {
                return index
                    .credentials
                    .get(&index.default_credential_name)
                    .cloned()
                    .map(|entry| (index.default_credential_name.clone(), entry));
            }
        }
        None
    }
}

pub fn sanitize_component(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    out.trim_matches(['.', '_', '-']).to_string()
}

pub fn sanitize_identity_name(raw: &str) -> String {
    sanitize_component(&raw.trim().to_ascii_lowercase())
}

pub fn preferred_dir_name(unique_id: &str) -> Result<String, IdentityError> {
    let value = sanitize_component(unique_id);
    if value.is_empty() {
        return Err(IdentityError::InvalidInput(
            "invalid input: unique_id is required".to_string(),
        ));
    }
    Ok(value)
}

pub fn ensure_dir(path: &Path) -> Result<(), IdentityError> {
    fs::create_dir_all(path)?;
    set_private_dir_mode(path)?;
    Ok(())
}

pub fn write_secure_json<T: Serialize>(path: &str, payload: &T) -> Result<(), IdentityError> {
    let raw = serde_json::to_vec_pretty(payload)?;
    fs::write(path, raw)?;
    set_private_file_mode(Path::new(path))?;
    Ok(())
}

pub fn write_secure_text(path: &str, payload: &str) -> Result<(), IdentityError> {
    fs::write(path, payload)?;
    set_private_file_mode(Path::new(path))?;
    Ok(())
}

pub fn read_json_value(path: &str) -> Result<serde_json::Value, IdentityError> {
    let raw = fs::read(path)?;
    Ok(serde_json::from_slice(&raw)?)
}

pub fn read_text(path: &str) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

pub fn file_exists(path: &str) -> bool {
    Path::new(path).is_file()
}

pub fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn load_index_from(path: &Path) -> Result<IndexPayload, IdentityError> {
    let raw = match fs::read(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(IndexPayload::default());
        }
        Err(err) => return Err(IdentityError::Io(err)),
    };
    let payload: IndexPayload = serde_json::from_slice(&raw)?;
    if !matches!(payload.schema_version, 0 | 2 | INDEX_SCHEMA_VERSION) {
        return Err(IdentityError::InvalidInput(format!(
            "unsupported identity index schema version: {}",
            payload.schema_version
        )));
    }
    Ok(normalize_index_payload(payload))
}

fn save_index_to(path: &Path, payload: IndexPayload) -> Result<(), IdentityError> {
    let normalized = normalize_index_payload(payload);
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(&normalized)?)?;
    set_private_file_mode(path)?;
    Ok(())
}

fn normalize_index_payload(mut payload: IndexPayload) -> IndexPayload {
    if payload.schema_version == 0 {
        payload.schema_version = INDEX_SCHEMA_VERSION;
    }
    if payload.default_credential_name.is_empty() && payload.credentials.contains_key("default") {
        payload.default_credential_name = "default".to_string();
    }
    let default_name = payload.default_credential_name.clone();
    let names = payload.credentials.keys().cloned().collect::<Vec<_>>();
    for name in names {
        if let Some(entry) = payload.credentials.get_mut(&name) {
            entry.credential_name = name.clone();
            entry.is_default = default_name == name;
        }
    }
    payload
}

pub fn copy_dir(src: &Path, dst: &Path) -> Result<(), IdentityError> {
    ensure_dir(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let source = entry.path();
        let target = dst.join(entry.file_name());
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            copy_dir(&source, &target)?;
        } else {
            if let Some(parent) = target.parent() {
                ensure_dir(parent)?;
            }
            fs::copy(&source, &target)?;
            fs::set_permissions(&target, metadata.permissions())?;
        }
    }
    Ok(())
}

pub fn unique_workspace_child(base: &Path, name: &str) -> PathBuf {
    let mut candidate = base.join(name);
    for idx in 2..1000 {
        if !candidate.exists() {
            return candidate;
        }
        candidate = base.join(format!("{name}-{idx}"));
    }
    candidate
}

#[cfg(unix)]
fn set_private_dir_mode(path: &Path) -> Result<(), IdentityError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_dir_mode(_path: &Path) -> Result<(), IdentityError> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file_mode(path: &Path) -> Result<(), IdentityError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_file_mode(_path: &Path) -> Result<(), IdentityError> {
    Ok(())
}
