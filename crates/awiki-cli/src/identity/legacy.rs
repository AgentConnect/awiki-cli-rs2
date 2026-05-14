use super::layout::{
    copy_dir, ensure_dir, file_exists, path_string, preferred_dir_name, read_json_value,
};
use super::store::copy_optional_legacy_e2ee_state;
use super::types::{
    IdentityError, IdentitySummary, ImportResult, IndexEntry, LegacyFlatIdentity, LegacyScan,
    SaveInput, INDEX_FILE_NAME, LEGACY_E2EE_PREFIX, LEGACY_LAYOUT_HINT,
};
use super::Manager;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

impl Manager {
    pub fn scan_legacy(&self) -> Result<LegacyScan, IdentityError> {
        let root = self.legacy_root_dir().to_string();
        let mut scan = LegacyScan {
            root_dir: root.clone(),
            indexed_layout: false,
            indexed_entries: BTreeMap::new(),
            legacy_credentials: Vec::new(),
            invalid_json_files: Vec::new(),
            orphan_e2ee_files: Vec::new(),
            has_legacy: false,
            hint: LEGACY_LAYOUT_HINT.to_string(),
        };
        if root.trim().is_empty() {
            return Ok(scan);
        }
        let root_path = Path::new(&root);
        if !root_path.is_dir() {
            return Ok(scan);
        }
        let index_path = root_path.join(INDEX_FILE_NAME);
        if index_path.is_file() {
            let index = read_legacy_index(&index_path)?;
            scan.indexed_layout = true;
            scan.indexed_entries = index.credentials;
        }

        let mut e2ee_candidates = BTreeMap::new();
        for entry in fs::read_dir(root_path)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.ends_with(".json") || name == INDEX_FILE_NAME {
                continue;
            }
            if name.ends_with("_did_document.json") {
                continue;
            }
            if name.starts_with(LEGACY_E2EE_PREFIX) {
                let credential = name
                    .trim_start_matches(LEGACY_E2EE_PREFIX)
                    .trim_end_matches(".json")
                    .to_string();
                e2ee_candidates.insert(credential, path_string(&entry.path()));
                continue;
            }
            let path = entry.path();
            let value = match read_json_value(path.to_string_lossy().as_ref()) {
                Ok(value) => value,
                Err(err) => {
                    scan.invalid_json_files.push(map2(
                        "file",
                        &name,
                        "reason",
                        &format!("invalid_json: {err}"),
                    ));
                    continue;
                }
            };
            let did = string_value(&value, "did", "");
            let private_key = string_value(&value, "private_key_pem", "");
            if did.is_empty() || private_key.is_empty() {
                scan.invalid_json_files.push(map2(
                    "file",
                    &name,
                    "reason",
                    "not_a_legacy_credential_payload",
                ));
                continue;
            }
            let mut unique_id = string_value(&value, "unique_id", "");
            if unique_id.is_empty() {
                unique_id = did.rsplit(':').next().unwrap_or_default().to_string();
            }
            scan.legacy_credentials.push(LegacyFlatIdentity {
                credential_name: name.trim_end_matches(".json").to_string(),
                path: path_string(&path),
                did,
                unique_id,
                handle: string_value(&value, "handle", ""),
            });
        }
        scan.legacy_credentials
            .sort_by(|left, right| left.credential_name.cmp(&right.credential_name));
        for (name, path) in e2ee_candidates {
            if scan
                .legacy_credentials
                .iter()
                .any(|legacy| legacy.credential_name == name)
            {
                continue;
            }
            scan.orphan_e2ee_files
                .push(map2("credential_name", &name, "file", &path));
        }
        scan.has_legacy = scan.indexed_layout
            || !scan.legacy_credentials.is_empty()
            || !scan.invalid_json_files.is_empty()
            || !scan.orphan_e2ee_files.is_empty();
        Ok(scan)
    }

    pub fn import_legacy(&self, mut name: String) -> Result<ImportResult, IdentityError> {
        let scan = self.scan_legacy()?;
        if !scan.has_legacy {
            return Err(IdentityError::LegacyNotFound(
                "legacy identity not found: no legacy layout detected".to_string(),
            ));
        }
        if name.trim().is_empty() {
            if scan.indexed_entries.len() == 1 {
                name = scan
                    .indexed_entries
                    .keys()
                    .next()
                    .cloned()
                    .unwrap_or_default();
            } else if scan.legacy_credentials.len() == 1 {
                name = scan.legacy_credentials[0].credential_name.clone();
            } else {
                return Err(IdentityError::InvalidInput(
                    "invalid input: multiple legacy identities detected, specify --name or --all"
                        .to_string(),
                ));
            }
        }
        let mut result = ImportResult::default();
        if let Some(entry) = scan.indexed_entries.get(&name) {
            result
                .imported
                .push(self.import_indexed_entry(&name, entry, &scan)?);
            return Ok(result);
        }
        if let Some(legacy) = scan
            .legacy_credentials
            .iter()
            .find(|legacy| legacy.credential_name == name)
        {
            result.imported.push(self.import_flat_legacy(legacy)?);
            return Ok(result);
        }
        Err(IdentityError::LegacyNotFound(format!(
            "legacy identity not found: {name}"
        )))
    }

    pub fn import_all_legacy(&self) -> Result<ImportResult, IdentityError> {
        let scan = self.scan_legacy()?;
        if !scan.has_legacy {
            return Err(IdentityError::LegacyNotFound(
                "legacy identity not found: no legacy layout detected".to_string(),
            ));
        }
        let mut result = ImportResult::default();
        for (name, entry) in &scan.indexed_entries {
            match self.import_indexed_entry(name, entry, &scan) {
                Ok(summary) => result.imported.push(summary),
                Err(IdentityError::Conflict(_)) => result.skipped.push(name.clone()),
                Err(err) => return Err(err),
            }
        }
        for legacy in &scan.legacy_credentials {
            match self.import_flat_legacy(legacy) {
                Ok(summary) => result.imported.push(summary),
                Err(IdentityError::Conflict(_)) => {
                    result.skipped.push(legacy.credential_name.clone())
                }
                Err(err) => return Err(err),
            }
        }
        Ok(result)
    }

    fn import_indexed_entry(
        &self,
        name: &str,
        entry: &IndexEntry,
        scan: &LegacyScan,
    ) -> Result<IdentitySummary, IdentityError> {
        let mut index = self.load_index()?;
        if let Some(existing) = index.credentials.get(name) {
            if existing.did != entry.did {
                return Err(IdentityError::Conflict(format!(
                    "identity conflict: identity {name} already exists"
                )));
            }
        }
        let src = Path::new(&scan.root_dir).join(&entry.dir_name);
        let dst = self.build_paths(&entry.dir_name);
        ensure_dir(Path::new(self.root_dir()))?;
        copy_dir(&src, Path::new(&dst.identity_dir))?;
        let mut stored = entry.clone();
        stored.credential_name = name.to_string();
        if index.default_credential_name.is_empty()
            && scan.indexed_layout
            && legacy_default_name(&Path::new(&scan.root_dir).join(INDEX_FILE_NAME))? == name
        {
            index.default_credential_name = name.to_string();
            stored.is_default = true;
        }
        index.credentials.insert(name.to_string(), stored.clone());
        self.save_index(index.clone())?;
        self.summary_for(&stored, &index.default_credential_name)
    }

    fn import_flat_legacy(
        &self,
        legacy: &LegacyFlatIdentity,
    ) -> Result<IdentitySummary, IdentityError> {
        let index = self.load_index()?;
        if let Some(existing) = index.credentials.get(&legacy.credential_name) {
            if existing.did != legacy.did {
                return Err(IdentityError::Conflict(format!(
                    "identity conflict: identity {} already exists",
                    legacy.credential_name
                )));
            }
        }
        let payload = read_json_value(&legacy.path)?;
        let dir_name = preferred_dir_name(&legacy.unique_id)?;
        let record = self.save(SaveInput {
            identity_name: legacy.credential_name.clone(),
            did: legacy.did.clone(),
            unique_id: legacy.unique_id.clone(),
            user_id: string_value(&payload, "user_id", ""),
            display_name: string_value(&payload, "name", ""),
            handle: string_value(&payload, "handle", &legacy.handle),
            jwt_token: string_value(&payload, "jwt_token", ""),
            did_document: payload.get("did_document").cloned(),
            key1_private_pem: string_value(&payload, "private_key_pem", ""),
            key1_public_pem: string_value(&payload, "public_key_pem", ""),
            e2ee_signing_private_pem: string_value(&payload, "e2ee_signing_private_pem", ""),
            e2ee_agreement_private_pem: string_value(&payload, "e2ee_agreement_private_pem", ""),
            ..SaveInput::default()
        })?;
        let e2ee_state_path = Path::new(&legacy.path)
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .join(format!(
                "{}{}.json",
                LEGACY_E2EE_PREFIX, legacy.credential_name
            ));
        if file_exists(e2ee_state_path.to_string_lossy().as_ref()) {
            let dst = self.build_paths(&dir_name);
            copy_optional_legacy_e2ee_state(&e2ee_state_path, &dst.e2ee_state_path);
        }
        let mut summary = super::store::identity_summary_from_record(&record);
        summary.user_state = super::store::evaluate_identity_summary_user_state(&summary);
        Ok(summary)
    }
}

fn read_legacy_index(path: &Path) -> Result<super::types::IndexPayload, IdentityError> {
    let value = read_json_value(path.to_string_lossy().as_ref())?;
    Ok(serde_json::from_value(value)?)
}

fn legacy_default_name(path: &Path) -> Result<String, IdentityError> {
    let payload = read_legacy_index(path)?;
    Ok(payload.default_credential_name)
}

fn string_value(value: &Value, key: &str, fallback: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

fn map2(k1: &str, v1: &str, k2: &str, v2: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        (k1.to_string(), v1.to_string()),
        (k2.to_string(), v2.to_string()),
    ])
}
