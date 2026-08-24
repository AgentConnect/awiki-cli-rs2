use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::{rngs::OsRng, RngCore as _};
use serde::{Deserialize, Serialize};

use crate::error::{SafeError, SafeResult};

const METADATA_SCHEMA_VERSION: u32 = 1;
const VAULT_DIRECTORY: &str = "vault";
const VAULT_ROOT_KEY_FILE: &str = "root-key.b64u";
const VAULT_WORKSPACE_ID: &str = "awiki-im-core-node";
const VAULT_CONTEXT_DEVICE_ID: &str = "primary";

#[derive(Debug)]
pub(crate) struct StateRoot {
    root: PathBuf,
    _lock: File,
    metadata: Arc<CompatibilityMetadataStore>,
}

impl StateRoot {
    pub(crate) fn open(root: impl Into<PathBuf>) -> SafeResult<Self> {
        let root = root.into();
        if !root.is_absolute() {
            return Err(SafeError::new(
                "invalid_state_root",
                "The IM state root must be an absolute path.",
                false,
            ));
        }
        create_private_dir(&root)?;
        for relative in ["identities", "local", "cache", "tmp", VAULT_DIRECTORY] {
            create_private_dir(&root.join(relative))?;
        }

        let lock_path = root.join(".awiki-im-core-node.lock");
        let lock = open_private_file(&lock_path)?;
        match fs2::FileExt::try_lock_exclusive(&lock) {
            Ok(()) => {}
            Err(error) if is_lock_contention(&error) => {
                return Err(SafeError::state_in_use());
            }
            Err(_) => return Err(SafeError::internal()),
        }
        let metadata = Arc::new(CompatibilityMetadataStore::new(
            root.join("compatibility.json"),
        ));
        Ok(Self {
            root,
            _lock: lock,
            metadata,
        })
    }

    pub(crate) fn paths(&self) -> im_core::ImCorePaths {
        im_core::ImCorePaths {
            identities: im_core::IdentityRegistryPaths {
                identity_root_dir: self.root.join("identities"),
                registry_path: self.root.join("identities/registry.json"),
                default_identity_path: Some(self.root.join("identities/default")),
            },
            local_state: im_core::LocalStatePaths {
                sqlite_path: self.root.join("local/im-core.sqlite3"),
            },
            runtime: im_core::RuntimePaths {
                cache_dir: self.root.join("cache"),
                temp_dir: self.root.join("tmp"),
            },
        }
    }

    pub(crate) fn metadata(&self) -> Arc<CompatibilityMetadataStore> {
        self.metadata.clone()
    }

    pub(crate) fn identity_vault_options(&self) -> SafeResult<im_core::ImCoreSecretVaultOptions> {
        let vault_dir = self.root.join(VAULT_DIRECTORY);
        create_private_dir(&vault_dir)?;
        let root_key = load_or_create_vault_root_key(&vault_dir.join(VAULT_ROOT_KEY_FILE))?;
        Ok(im_core::ImCoreSecretVaultOptions::new(
            root_key,
            vault_dir,
            VAULT_WORKSPACE_ID,
            VAULT_CONTEXT_DEVICE_ID,
        ))
    }

    pub(crate) fn harden_permissions(&self) -> SafeResult<()> {
        harden_tree(&self.root)
    }

    pub(crate) fn clear_owned_data(&self) -> SafeResult<bool> {
        let root_metadata = fs::symlink_metadata(&self.root).map_err(|_| SafeError::internal())?;
        if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
            return Err(SafeError::internal());
        }
        let lock_path = self.root.join(".awiki-im-core-node.lock");
        let lock_metadata = fs::symlink_metadata(lock_path).map_err(|_| SafeError::internal())?;
        if lock_metadata.file_type().is_symlink() || !lock_metadata.is_file() {
            return Err(SafeError::internal());
        }
        let mut cleared = false;
        for relative in ["identities", "local", "cache", "tmp", VAULT_DIRECTORY] {
            let path = self.root.join(relative);
            match fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                    return Err(SafeError::internal());
                }
                Ok(_) => {
                    if fs::read_dir(&path)
                        .map_err(|_| SafeError::internal())?
                        .next()
                        .is_some()
                    {
                        cleared = true;
                    }
                    fs::remove_dir_all(&path).map_err(|_| SafeError::internal())?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(SafeError::internal()),
            }
            create_private_dir(&path)?;
        }

        let metadata_path = self.root.join("compatibility.json");
        match fs::symlink_metadata(&metadata_path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                return Err(SafeError::internal());
            }
            Ok(_) => {
                fs::remove_file(&metadata_path).map_err(|_| SafeError::internal())?;
                cleared = true;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(SafeError::internal()),
        }
        self.harden_permissions()?;
        Ok(cleared)
    }
}

fn is_lock_contention(error: &std::io::Error) -> bool {
    if error.kind() == std::io::ErrorKind::WouldBlock {
        return true;
    }
    #[cfg(windows)]
    {
        // LockFileEx reports ERROR_LOCK_VIOLATION when another process owns
        // the byte-range lock. Some Windows filesystems surface the adjacent
        // sharing violation instead. Rust does not consistently classify
        // either value as WouldBlock across supported toolchains.
        return is_windows_lock_contention(error.raw_os_error());
    }
    #[cfg(not(windows))]
    false
}

#[cfg(any(windows, test))]
fn is_windows_lock_contention(raw_os_error: Option<i32>) -> bool {
    matches!(raw_os_error, Some(32 | 33))
}

#[derive(Debug)]
pub(crate) struct CompatibilityMetadataStore {
    path: PathBuf,
    access: Mutex<()>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CompatibilityMetadata {
    schema_version: u32,
    registered_at_ms_by_identity: BTreeMap<String, String>,
}

impl Default for CompatibilityMetadata {
    fn default() -> Self {
        Self {
            schema_version: METADATA_SCHEMA_VERSION,
            registered_at_ms_by_identity: BTreeMap::new(),
        }
    }
}

impl CompatibilityMetadataStore {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            access: Mutex::new(()),
        }
    }

    pub(crate) fn ensure_registered_at(&self, identity_id: &str) -> SafeResult<String> {
        self.ensure_registered_at_with(identity_id, unix_time_millis()?)
    }

    fn ensure_registered_at_with(
        &self,
        identity_id: &str,
        registered_at_ms: String,
    ) -> SafeResult<String> {
        let _guard = self.access.lock().map_err(|_| SafeError::internal())?;
        let mut metadata = self.load()?;
        if let Some(existing) = metadata.registered_at_ms_by_identity.get(identity_id) {
            return Ok(existing.clone());
        }
        metadata
            .registered_at_ms_by_identity
            .insert(identity_id.to_owned(), registered_at_ms.clone());
        self.store(&metadata)?;
        Ok(registered_at_ms)
    }

    fn load(&self) -> SafeResult<CompatibilityMetadata> {
        if !self.path.exists() {
            return Ok(CompatibilityMetadata::default());
        }
        let mut file = File::open(&self.path).map_err(|_| SafeError::internal())?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|_| SafeError::internal())?;
        let metadata: CompatibilityMetadata =
            serde_json::from_slice(&bytes).map_err(|_| SafeError::internal())?;
        if metadata.schema_version != METADATA_SCHEMA_VERSION {
            return Err(SafeError::internal());
        }
        Ok(metadata)
    }

    fn store(&self, metadata: &CompatibilityMetadata) -> SafeResult<()> {
        let parent = self.path.parent().ok_or_else(SafeError::internal)?;
        create_private_dir(parent)?;
        let temporary = self.path.with_extension(format!(
            "tmp-{}-{}",
            std::process::id(),
            unix_time_millis()?
        ));
        let bytes = serde_json::to_vec(metadata).map_err(|_| SafeError::internal())?;
        let result = (|| -> SafeResult<()> {
            let mut file = create_private_file(&temporary)?;
            file.write_all(&bytes).map_err(|_| SafeError::internal())?;
            file.sync_all().map_err(|_| SafeError::internal())?;
            #[cfg(windows)]
            if self.path.exists() {
                fs::remove_file(&self.path).map_err(|_| SafeError::internal())?;
            }
            fs::rename(&temporary, &self.path).map_err(|_| SafeError::internal())?;
            set_private_file_mode(&self.path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

fn unix_time_millis() -> SafeResult<String> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| SafeError::internal())?
        .as_millis();
    Ok(millis.to_string())
}

fn load_or_create_vault_root_key(path: &Path) -> SafeResult<im_core::vault::DeviceVaultRootKey> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(SafeError::internal());
        }
        Ok(_) => {
            let encoded = fs::read_to_string(path).map_err(|_| SafeError::internal())?;
            let decoded = URL_SAFE_NO_PAD
                .decode(encoded.trim())
                .map_err(|_| SafeError::internal())?;
            let bytes: [u8; im_core::vault::DEVICE_VAULT_ROOT_KEY_LEN] =
                decoded.try_into().map_err(|_| SafeError::internal())?;
            set_private_file_mode(path)?;
            return Ok(im_core::vault::DeviceVaultRootKey::from_bytes(bytes));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(SafeError::internal()),
    }

    let mut bytes = [0_u8; im_core::vault::DEVICE_VAULT_ROOT_KEY_LEN];
    OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| SafeError::internal())?;
    let encoded = URL_SAFE_NO_PAD.encode(bytes);
    let result = (|| -> SafeResult<()> {
        let mut file = create_private_file(path)?;
        file.write_all(encoded.as_bytes())
            .map_err(|_| SafeError::internal())?;
        file.write_all(b"\n").map_err(|_| SafeError::internal())?;
        file.sync_all().map_err(|_| SafeError::internal())?;
        set_private_file_mode(path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result?;
    Ok(im_core::vault::DeviceVaultRootKey::from_bytes(bytes))
}

fn create_private_dir(path: &Path) -> SafeResult<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(SafeError::internal());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(path).map_err(|_| SafeError::internal())?;
        }
        Err(_) => return Err(SafeError::internal()),
    }
    set_private_dir_mode(path)
}

fn open_private_file(path: &Path) -> SafeResult<File> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(SafeError::internal());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(SafeError::internal()),
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let file = options.open(path).map_err(|_| SafeError::internal())?;
    set_private_file_mode(path)?;
    Ok(file)
}

fn create_private_file(path: &Path) -> SafeResult<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let file = options.open(path).map_err(|_| SafeError::internal())?;
    set_private_file_mode(path)?;
    Ok(file)
}

fn harden_tree(path: &Path) -> SafeResult<()> {
    let metadata = fs::symlink_metadata(path).map_err(|_| SafeError::internal())?;
    if metadata.file_type().is_symlink() {
        return Err(SafeError::internal());
    }
    if metadata.is_dir() {
        set_private_dir_mode(path)?;
        for entry in fs::read_dir(path).map_err(|_| SafeError::internal())? {
            harden_tree(&entry.map_err(|_| SafeError::internal())?.path())?;
        }
    } else if metadata.is_file() {
        set_private_file_mode(path)?;
    }
    Ok(())
}

#[cfg(unix)]
fn set_private_dir_mode(path: &Path) -> SafeResult<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|_| SafeError::internal())
}

#[cfg(not(unix))]
fn set_private_dir_mode(_path: &Path) -> SafeResult<()> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file_mode(path: &Path) -> SafeResult<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|_| SafeError::internal())
}

#[cfg(not(unix))]
fn set_private_file_mode(_path: &Path) -> SafeResult<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_root_is_exclusive_and_uses_private_unix_permissions() {
        let directory = tempfile::tempdir().unwrap();
        let first = StateRoot::open(directory.path().to_path_buf()).unwrap();
        let second = StateRoot::open(directory.path().to_path_buf()).unwrap_err();
        assert_eq!(second.code, "state_in_use");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                fs::metadata(directory.path()).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(directory.path().join(".awiki-im-core-node.lock"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        drop(first);
        StateRoot::open(directory.path().to_path_buf()).unwrap();
    }

    #[test]
    fn windows_lock_violations_are_reported_as_contention() {
        assert!(is_windows_lock_contention(Some(32)));
        assert!(is_windows_lock_contention(Some(33)));
        assert!(!is_windows_lock_contention(Some(5)));
        assert!(!is_windows_lock_contention(None));
    }

    #[test]
    fn registered_at_is_stable_across_store_recreation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("compatibility.json");
        let first = CompatibilityMetadataStore::new(path.clone());
        assert_eq!(
            first.ensure_registered_at_with("identity-1", "1000".to_owned()),
            Ok("1000".to_owned())
        );
        let restarted = CompatibilityMetadataStore::new(path);
        assert_eq!(
            restarted.ensure_registered_at_with("identity-1", "2000".to_owned()),
            Ok("1000".to_owned())
        );
    }

    #[test]
    fn vault_root_key_is_private_stable_and_rotated_by_clear() {
        let directory = tempfile::tempdir().unwrap();
        let state = StateRoot::open(directory.path().to_path_buf()).unwrap();
        state.identity_vault_options().unwrap();
        let root_key_path = directory
            .path()
            .join(VAULT_DIRECTORY)
            .join(VAULT_ROOT_KEY_FILE);
        let first = fs::read(&root_key_path).unwrap();
        assert_eq!(first.len(), 44);

        state.identity_vault_options().unwrap();
        assert_eq!(fs::read(&root_key_path).unwrap(), first);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                fs::metadata(&root_key_path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }

        assert!(state.clear_owned_data().unwrap());
        assert!(!root_key_path.exists());
        state.identity_vault_options().unwrap();
        assert_ne!(fs::read(root_key_path).unwrap(), first);
    }

    #[cfg(unix)]
    #[test]
    fn state_root_rejects_symlinked_runtime_paths_and_lock_files() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), directory.path().join("cache")).unwrap();
        assert_eq!(
            StateRoot::open(directory.path().to_path_buf())
                .unwrap_err()
                .code,
            "internal"
        );

        let directory = tempfile::tempdir().unwrap();
        let outside_lock = outside.path().join("outside.lock");
        File::create(&outside_lock).unwrap();
        symlink(
            &outside_lock,
            directory.path().join(".awiki-im-core-node.lock"),
        )
        .unwrap();
        assert_eq!(
            StateRoot::open(directory.path().to_path_buf())
                .unwrap_err()
                .code,
            "internal"
        );
    }
}
