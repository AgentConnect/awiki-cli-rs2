use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::Deserialize;

pub struct IdentityRegistry<'a> {
    core: &'a crate::core::ImCore,
}

impl<'a> IdentityRegistry<'a> {
    pub(crate) fn new(core: &'a crate::core::ImCore) -> Self {
        Self { core }
    }

    pub fn list(&self) -> crate::ImResult<Vec<super::IdentitySummary>> {
        Ok(self
            .load_registry()?
            .entries
            .into_iter()
            .map(|entry| entry.summary)
            .collect())
    }

    pub fn default_identity(&self) -> crate::ImResult<Option<super::IdentitySummary>> {
        Ok(self.load_registry()?.default_identity())
    }

    pub fn resolve(
        &self,
        selector: super::IdentitySelector,
    ) -> crate::ImResult<super::IdentitySummary> {
        let registry = self.load_registry()?;
        match selector {
            super::IdentitySelector::Default => registry
                .default_identity()
                .ok_or(crate::ImError::DefaultIdentityMissing),
            super::IdentitySelector::LocalAlias(alias) => {
                let alias = alias.trim();
                if alias.is_empty() {
                    return Err(crate::ImError::invalid_input(
                        Some("identity".to_string()),
                        "local alias must not be empty",
                    ));
                }
                registry
                    .find(|entry| entry.local_alias.as_deref() == Some(alias))
                    .map(|entry| entry.summary.clone())
                    .map_or_else(
                        || {
                            if registry.entries.is_empty() {
                                self.summary_for_local_alias(alias.to_string())
                            } else {
                                Err(crate::ImError::IdentityNotFound {
                                    selector: alias.to_string(),
                                })
                            }
                        },
                        Ok,
                    )
            }
            super::IdentitySelector::Did(did) => registry
                .find(|entry| entry.summary.did == did)
                .map(|entry| entry.summary.clone())
                .map_or_else(
                    || {
                        if registry.entries.is_empty() {
                            self.summary_for_did(did)
                        } else {
                            Err(crate::ImError::IdentityNotFound {
                                selector: did.as_str().to_string(),
                            })
                        }
                    },
                    Ok,
                ),
            super::IdentitySelector::Id(id) => registry
                .find(|entry| entry.summary.id == id)
                .map(|entry| entry.summary.clone())
                .ok_or_else(|| crate::ImError::IdentityNotFound {
                    selector: id.as_str().to_string(),
                }),
            super::IdentitySelector::Handle(handle) => registry
                .find(|entry| entry.summary.handle.as_ref() == Some(&handle))
                .map(|entry| entry.summary.clone())
                .ok_or_else(|| crate::ImError::IdentityNotFound {
                    selector: handle.as_str().to_string(),
                }),
        }
    }

    fn summary_for_local_alias(&self, alias: String) -> crate::ImResult<super::IdentitySummary> {
        let alias = alias.trim();
        if alias.is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("identity".to_string()),
                "local alias must not be empty",
            ));
        }
        let did = crate::ids::Did::parse(format!(
            "did:awiki:{}",
            alias
                .chars()
                .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
                .collect::<String>()
        ))?;
        Ok(super::IdentitySummary {
            id: crate::ids::IdentityId::parse(alias)?,
            did,
            handle: None,
            display_name: None,
            local_alias: Some(alias.to_string()),
            device_id: None,
            is_default: false,
            readiness: super::IdentityReadiness {
                ready_for_auth: false,
                ready_for_messaging: false,
                missing: vec![
                    super::IdentityMissingItem::DidDocument,
                    super::IdentityMissingItem::PrivateKey,
                    super::IdentityMissingItem::AuthState,
                ],
            },
        })
    }

    fn summary_for_did(&self, did: crate::ids::Did) -> crate::ImResult<super::IdentitySummary> {
        let id = did.as_str().replace(':', "-");
        Ok(super::IdentitySummary {
            id: crate::ids::IdentityId::parse(id)?,
            did,
            handle: None,
            display_name: None,
            local_alias: None,
            device_id: None,
            is_default: false,
            readiness: super::IdentityReadiness {
                ready_for_auth: false,
                ready_for_messaging: false,
                missing: vec![
                    super::IdentityMissingItem::DidDocument,
                    super::IdentityMissingItem::PrivateKey,
                    super::IdentityMissingItem::AuthState,
                ],
            },
        })
    }

    pub fn register_handle(
        &self,
        request: super::RegisterHandleRequest,
    ) -> crate::ImResult<super::HandleRegistrationResult> {
        let alias = request
            .local_alias
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| local_part(request.requested_handle.as_str()));
        let did = crate::ids::Did::parse(format!(
            "did:awiki:{}",
            sanitize_did_suffix(request.requested_handle.as_str())
        ))?;
        let identity = super::IdentitySummary {
            id: crate::ids::IdentityId::parse(alias)?,
            did,
            handle: Some(request.requested_handle.clone()),
            display_name: request.profile.display_name.clone(),
            local_alias: Some(alias.to_string()),
            device_id: None,
            is_default: request.make_default,
            readiness: super::IdentityReadiness {
                ready_for_auth: true,
                ready_for_messaging: true,
                missing: Vec::new(),
            },
        };
        let default_identity_change = request.make_default.then(|| super::DefaultIdentityChange {
            previous: self.default_identity().ok().flatten(),
            next: identity.clone(),
            requires_default_identity_write: true,
            warnings: Vec::new(),
        });
        Ok(super::HandleRegistrationResult {
            identity,
            default_identity_change,
            warnings: Vec::new(),
        })
    }

    pub fn plan_default_identity_change(
        &self,
        selector: super::IdentitySelector,
    ) -> crate::ImResult<super::DefaultIdentityChange> {
        let previous = self.default_identity()?;
        let next = self.resolve(selector)?;
        Ok(super::DefaultIdentityChange {
            previous,
            next,
            requires_default_identity_write: true,
            warnings: Vec::new(),
        })
    }

    pub(crate) fn load_runtime(
        &self,
        selector: super::IdentitySelector,
    ) -> crate::ImResult<crate::internal::identity_runtime::ClientIdentityRuntime> {
        let summary = self.resolve(selector)?;
        let identity_root = &self.core.inner().sdk_paths().identities.identity_root_dir;
        let alias = summary
            .local_alias
            .as_deref()
            .unwrap_or_else(|| summary.id.as_str());
        Ok(crate::internal::identity_runtime::ClientIdentityRuntime {
            summary: summary.clone(),
            did_document_path: identity_root.join(alias).join("did.json"),
            private_key_path: identity_root.join(alias).join("private.key"),
            auth_state_path: identity_root.join(alias).join("auth.json"),
            owner: crate::internal::identity_runtime::LocalOwnerContext {
                identity_id: summary.id,
                current_did: summary.did,
            },
        })
    }

    fn load_registry(&self) -> crate::ImResult<RegistrySnapshot> {
        let paths = &self.core.inner().sdk_paths().identities;
        let mut snapshot = match fs::read(&paths.registry_path) {
            Ok(raw) => parse_registry(&raw)?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => RegistrySnapshot {
                default_alias: default_alias_from_file(paths.default_identity_path.as_deref())?,
                entries: Vec::new(),
            },
            Err(err) => {
                return Err(crate::ImError::CredentialFileUnreadable {
                    path_kind: "identity_registry".to_string(),
                    detail: err.to_string(),
                });
            }
        };
        if snapshot.default_alias.is_none() {
            snapshot.default_alias =
                default_alias_from_file(paths.default_identity_path.as_deref())?;
        }
        snapshot.apply_default_flags();
        Ok(snapshot)
    }
}

#[derive(Debug, Clone)]
struct RegistrySnapshot {
    default_alias: Option<String>,
    entries: Vec<RegistryEntry>,
}

impl RegistrySnapshot {
    fn default_identity(&self) -> Option<super::IdentitySummary> {
        self.find(|entry| entry.summary.is_default)
            .map(|entry| entry.summary.clone())
            .or_else(|| {
                self.default_alias.as_deref().and_then(|alias| {
                    self.find(|entry| entry.local_alias.as_deref() == Some(alias))
                        .map(|entry| entry.summary.clone())
                })
            })
    }

    fn find(&self, predicate: impl Fn(&RegistryEntry) -> bool) -> Option<&RegistryEntry> {
        self.entries.iter().find(|entry| predicate(entry))
    }

    fn apply_default_flags(&mut self) {
        let default_alias = self.default_alias.clone();
        for entry in &mut self.entries {
            entry.summary.is_default = default_alias
                .as_deref()
                .is_some_and(|alias| entry.local_alias.as_deref() == Some(alias))
                || entry.summary.is_default;
        }
    }
}

#[derive(Debug, Clone)]
struct RegistryEntry {
    local_alias: Option<String>,
    summary: super::IdentitySummary,
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
    did: String,
    #[serde(default)]
    handle: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    local_alias: Option<String>,
    #[serde(default)]
    device_id: Option<String>,
    #[serde(default)]
    is_default: bool,
    #[serde(default)]
    ready_for_auth: bool,
    #[serde(default)]
    ready_for_messaging: bool,
    #[serde(default)]
    missing: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct LegacyRegistryFile {
    #[serde(default)]
    default_credential_name: String,
    #[serde(default)]
    credentials: BTreeMap<String, LegacyIdentityRecord>,
}

#[derive(Debug, Deserialize)]
struct LegacyIdentityRecord {
    #[serde(default)]
    credential_name: String,
    #[serde(default)]
    did: String,
    #[serde(default)]
    unique_id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    handle: String,
    #[serde(default)]
    full_handle: String,
    #[serde(default)]
    is_default: bool,
}

fn parse_registry(raw: &[u8]) -> crate::ImResult<RegistrySnapshot> {
    if let Ok(file) = serde_json::from_slice::<SdkRegistryFile>(raw) {
        if !file.identities.is_empty() {
            return sdk_registry_snapshot(file);
        }
    }
    let file: LegacyRegistryFile =
        serde_json::from_slice(raw).map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })?;
    legacy_registry_snapshot(file)
}

fn sdk_registry_snapshot(file: SdkRegistryFile) -> crate::ImResult<RegistrySnapshot> {
    let mut entries = Vec::with_capacity(file.identities.len());
    for record in file.identities {
        let local_alias = record
            .local_alias
            .clone()
            .or_else(|| Some(record.id.clone()).filter(|value| !value.trim().is_empty()));
        entries.push(RegistryEntry {
            local_alias,
            summary: super::IdentitySummary {
                id: crate::ids::IdentityId::parse(record.id)?,
                did: crate::ids::Did::parse(record.did)?,
                handle: optional_handle(record.handle)?,
                display_name: record.display_name,
                local_alias: record.local_alias,
                device_id: record.device_id,
                is_default: record.is_default,
                readiness: super::IdentityReadiness {
                    ready_for_auth: record.ready_for_auth,
                    ready_for_messaging: record.ready_for_messaging,
                    missing: record
                        .missing
                        .into_iter()
                        .map(identity_missing_item)
                        .collect(),
                },
            },
        });
    }
    Ok(RegistrySnapshot {
        default_alias: file.default_identity,
        entries,
    })
}

fn legacy_registry_snapshot(file: LegacyRegistryFile) -> crate::ImResult<RegistrySnapshot> {
    let mut entries = Vec::with_capacity(file.credentials.len());
    for (alias, record) in file.credentials {
        let id = first_non_empty([&record.unique_id, &record.credential_name, &alias])
            .unwrap_or(&alias)
            .to_string();
        let handle = first_non_empty([&record.full_handle, &record.handle, ""]);
        entries.push(RegistryEntry {
            local_alias: Some(alias.clone()),
            summary: super::IdentitySummary {
                id: crate::ids::IdentityId::parse(id)?,
                did: crate::ids::Did::parse(record.did)?,
                handle: optional_handle(handle.map(str::to_string))?,
                display_name: Some(record.name).filter(|value| !value.trim().is_empty()),
                local_alias: Some(alias),
                device_id: None,
                is_default: record.is_default,
                readiness: super::IdentityReadiness {
                    ready_for_auth: true,
                    ready_for_messaging: true,
                    missing: Vec::new(),
                },
            },
        });
    }
    Ok(RegistrySnapshot {
        default_alias: Some(file.default_credential_name).filter(|value| !value.trim().is_empty()),
        entries,
    })
}

fn default_alias_from_file(path: Option<&Path>) -> crate::ImResult<Option<String>> {
    let Some(path) = path else {
        return Ok(None);
    };
    match fs::read_to_string(path) {
        Ok(value) => Ok(Some(value.trim().to_string()).filter(|value| !value.is_empty())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(crate::ImError::CredentialFileUnreadable {
            path_kind: "default_identity".to_string(),
            detail: err.to_string(),
        }),
    }
}

fn optional_handle(value: Option<String>) -> crate::ImResult<Option<crate::ids::Handle>> {
    value
        .map(|value| value.trim().trim_start_matches('@').to_string())
        .filter(|value| !value.is_empty())
        .map(|value| crate::ids::Handle::parse(value, ""))
        .transpose()
}

fn identity_missing_item(value: String) -> super::IdentityMissingItem {
    match value.trim() {
        "did_document" | "DidDocument" => super::IdentityMissingItem::DidDocument,
        "private_key" | "PrivateKey" => super::IdentityMissingItem::PrivateKey,
        "auth_state" | "AuthState" => super::IdentityMissingItem::AuthState,
        "handle" | "Handle" => super::IdentityMissingItem::Handle,
        "message_endpoint" | "MessageEndpoint" => super::IdentityMissingItem::MessageEndpoint,
        other => super::IdentityMissingItem::Other(other.to_string()),
    }
}

fn local_part(handle: &str) -> &str {
    handle
        .trim_start_matches('@')
        .split('.')
        .next()
        .unwrap_or(handle)
}

fn sanitize_did_suffix(value: &str) -> String {
    value
        .trim_start_matches('@')
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

fn first_non_empty<'a, const N: usize>(values: [&'a str; N]) -> Option<&'a str> {
    values.into_iter().find(|value| !value.trim().is_empty())
}
