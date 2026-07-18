//! AWiki-local authorization projection for one protocol device.
//!
//! This module deliberately does not define ANP wire fields. The public
//! `deviceManifest` remains in the DID Document; role, readiness and internal
//! checkpoints stay in the authenticated first-party state described here.

use serde::{Deserialize, Serialize};

pub(crate) const IDENTITY_DEVICE_STATE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum IdentityDeviceMode {
    Legacy,
    VNext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DeviceAuthorizationStatus {
    Active,
    Revoked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DeviceAuthorizationRole {
    Member,
    Admin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DeviceAuthorizationProjection {
    pub(crate) protocol_device_id: crate::ids::ProtocolDeviceId,
    pub(crate) signing_key_id: String,
    pub(crate) e2ee_key_id: String,
    pub(crate) status: DeviceAuthorizationStatus,
    pub(crate) role: DeviceAuthorizationRole,
    pub(crate) management_ready: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct IdentityInternalCheckpoint {
    pub(crate) document_version: u64,
    pub(crate) document_hash: String,
    pub(crate) registry_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct IdentityDeviceState {
    pub(crate) schema_version: u32,
    pub(crate) mode: IdentityDeviceMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) authorization: Option<DeviceAuthorizationProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) checkpoint: Option<IdentityInternalCheckpoint>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LocalDeviceReadiness {
    Legacy,
    MemberReady,
    AdminAwaitingRoot,
    AdminReady,
    Blocked { reason: String },
}

impl IdentityDeviceState {
    pub(crate) fn legacy() -> Self {
        Self {
            schema_version: IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
            mode: IdentityDeviceMode::Legacy,
            authorization: None,
            checkpoint: None,
        }
    }

    pub(crate) fn validate_for_did(&self, did: &crate::ids::Did) -> crate::ImResult<()> {
        if self.schema_version != IDENTITY_DEVICE_STATE_SCHEMA_VERSION {
            return Err(crate::ImError::invalid_input(
                Some("identity_device_state.schema_version".to_owned()),
                "unsupported identity device state schema version",
            ));
        }

        match self.mode {
            IdentityDeviceMode::Legacy => {
                if self.authorization.is_some() || self.checkpoint.is_some() {
                    return Err(crate::ImError::invalid_input(
                        Some("identity_device_state".to_owned()),
                        "legacy identity state must not contain vNext authorization or checkpoint",
                    ));
                }
                Ok(())
            }
            IdentityDeviceMode::VNext => {
                let authorization = self.authorization.as_ref().ok_or_else(|| {
                    crate::ImError::invalid_input(
                        Some("identity_device_state.authorization".to_owned()),
                        "vNext identity state requires a device authorization projection",
                    )
                })?;
                let checkpoint = self.checkpoint.as_ref().ok_or_else(|| {
                    crate::ImError::invalid_input(
                        Some("identity_device_state.checkpoint".to_owned()),
                        "vNext identity state requires an internal checkpoint",
                    )
                })?;
                validate_authorization(authorization, did)?;
                validate_checkpoint(checkpoint)
            }
        }
    }

    pub(crate) fn readiness(
        &self,
        local_root_available: bool,
        local_blocker: Option<&str>,
    ) -> LocalDeviceReadiness {
        if let Some(reason) = local_blocker
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return LocalDeviceReadiness::Blocked {
                reason: reason.to_owned(),
            };
        }
        if self.schema_version != IDENTITY_DEVICE_STATE_SCHEMA_VERSION {
            return LocalDeviceReadiness::Blocked {
                reason: "device_state_schema_unsupported".to_owned(),
            };
        }
        if self.mode == IdentityDeviceMode::Legacy {
            return if self.authorization.is_none() && self.checkpoint.is_none() {
                LocalDeviceReadiness::Legacy
            } else {
                LocalDeviceReadiness::Blocked {
                    reason: "legacy_device_state_invalid".to_owned(),
                }
            };
        }
        let Some(authorization) = self.authorization.as_ref() else {
            return LocalDeviceReadiness::Blocked {
                reason: "device_authorization_missing".to_owned(),
            };
        };
        if self.checkpoint.is_none() {
            return LocalDeviceReadiness::Blocked {
                reason: "identity_checkpoint_missing".to_owned(),
            };
        }
        if authorization.status == DeviceAuthorizationStatus::Revoked {
            return LocalDeviceReadiness::Blocked {
                reason: "device_revoked".to_owned(),
            };
        }
        if authorization.management_ready && authorization.role != DeviceAuthorizationRole::Admin {
            return LocalDeviceReadiness::Blocked {
                reason: "member_management_ready_invalid".to_owned(),
            };
        }
        match authorization.role {
            DeviceAuthorizationRole::Member => LocalDeviceReadiness::MemberReady,
            DeviceAuthorizationRole::Admin
                if authorization.management_ready && local_root_available =>
            {
                LocalDeviceReadiness::AdminReady
            }
            DeviceAuthorizationRole::Admin => LocalDeviceReadiness::AdminAwaitingRoot,
        }
    }
}

fn validate_authorization(
    authorization: &DeviceAuthorizationProjection,
    did: &crate::ids::Did,
) -> crate::ImResult<()> {
    crate::ids::ProtocolDeviceId::parse(authorization.protocol_device_id.as_str())?;
    validate_key_id(did, "signing_key_id", &authorization.signing_key_id)?;
    validate_key_id(did, "e2ee_key_id", &authorization.e2ee_key_id)?;
    if authorization.signing_key_id == authorization.e2ee_key_id {
        return Err(crate::ImError::invalid_input(
            Some("identity_device_state.authorization".to_owned()),
            "device signing and E2EE key ids must be distinct",
        ));
    }
    if authorization.role == DeviceAuthorizationRole::Member && authorization.management_ready {
        return Err(crate::ImError::invalid_input(
            Some("identity_device_state.authorization.management_ready".to_owned()),
            "member device must not be management-ready",
        ));
    }
    if authorization.status == DeviceAuthorizationStatus::Revoked && authorization.management_ready
    {
        return Err(crate::ImError::invalid_input(
            Some("identity_device_state.authorization.management_ready".to_owned()),
            "revoked device must not be management-ready",
        ));
    }
    Ok(())
}

fn validate_key_id(did: &crate::ids::Did, field: &str, value: &str) -> crate::ImResult<()> {
    let expected_prefix = format!("{}#", did.as_str());
    if !value.starts_with(&expected_prefix) || value.len() == expected_prefix.len() {
        return Err(crate::ImError::invalid_input(
            Some(format!("identity_device_state.authorization.{field}")),
            format!("{field} must be a key id in the identity DID Document"),
        ));
    }
    Ok(())
}

fn validate_checkpoint(checkpoint: &IdentityInternalCheckpoint) -> crate::ImResult<()> {
    if checkpoint.document_version == 0 || checkpoint.registry_version == 0 {
        return Err(crate::ImError::invalid_input(
            Some("identity_device_state.checkpoint".to_owned()),
            "vNext document and registry versions must be positive",
        ));
    }
    let Some(encoded) = checkpoint.document_hash.strip_prefix("sha256:") else {
        return Err(crate::ImError::invalid_input(
            Some("identity_device_state.checkpoint.document_hash".to_owned()),
            "document hash must use sha256:base64url-no-padding form",
        ));
    };
    if encoded.len() != 43
        || encoded
            .bytes()
            .any(|value| !value.is_ascii_alphanumeric() && value != b'-' && value != b'_')
    {
        return Err(crate::ImError::invalid_input(
            Some("identity_device_state.checkpoint.document_hash".to_owned()),
            "document hash must use sha256:base64url-no-padding form",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vnext_state(role: DeviceAuthorizationRole, management_ready: bool) -> IdentityDeviceState {
        let did = "did:wba:awiki.info:user:alice:e1_root";
        IdentityDeviceState {
            schema_version: IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
            mode: IdentityDeviceMode::VNext,
            authorization: Some(DeviceAuthorizationProjection {
                protocol_device_id: crate::ids::ProtocolDeviceId::parse("dev-device-a").unwrap(),
                signing_key_id: format!("{did}#device-a-sign"),
                e2ee_key_id: format!("{did}#device-a-e2ee"),
                status: DeviceAuthorizationStatus::Active,
                role,
                management_ready,
            }),
            checkpoint: Some(IdentityInternalCheckpoint {
                document_version: 1,
                document_hash: "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned(),
                registry_version: 1,
            }),
        }
    }

    #[test]
    fn legacy_state_cannot_smuggle_vnext_projection() {
        let did = crate::ids::Did::parse("did:example:alice").unwrap();
        let mut state = IdentityDeviceState::legacy();
        state.checkpoint = Some(IdentityInternalCheckpoint {
            document_version: 1,
            document_hash: "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned(),
            registry_version: 1,
        });

        assert!(state.validate_for_did(&did).is_err());
        assert_eq!(
            IdentityDeviceState::legacy().readiness(false, None),
            LocalDeviceReadiness::Legacy
        );
    }

    #[test]
    fn readiness_requires_both_admin_ack_and_local_root() {
        let did = crate::ids::Did::parse("did:wba:awiki.info:user:alice:e1_root").unwrap();
        let awaiting = vnext_state(DeviceAuthorizationRole::Admin, false);
        let ready = vnext_state(DeviceAuthorizationRole::Admin, true);

        awaiting.validate_for_did(&did).unwrap();
        ready.validate_for_did(&did).unwrap();
        assert_eq!(
            awaiting.readiness(true, None),
            LocalDeviceReadiness::AdminAwaitingRoot
        );
        assert_eq!(
            ready.readiness(false, None),
            LocalDeviceReadiness::AdminAwaitingRoot
        );
        assert_eq!(
            ready.readiness(true, None),
            LocalDeviceReadiness::AdminReady
        );
    }

    #[test]
    fn member_ready_does_not_require_root_and_cannot_claim_management_ready() {
        let did = crate::ids::Did::parse("did:wba:awiki.info:user:alice:e1_root").unwrap();
        let member = vnext_state(DeviceAuthorizationRole::Member, false);
        let invalid = vnext_state(DeviceAuthorizationRole::Member, true);

        member.validate_for_did(&did).unwrap();
        assert_eq!(
            member.readiness(false, None),
            LocalDeviceReadiness::MemberReady
        );
        assert!(invalid.validate_for_did(&did).is_err());
        assert_eq!(
            invalid.readiness(false, None),
            LocalDeviceReadiness::Blocked {
                reason: "member_management_ready_invalid".to_owned()
            }
        );
    }

    #[test]
    fn revoked_or_locally_blocked_device_is_not_ready() {
        let mut revoked = vnext_state(DeviceAuthorizationRole::Admin, false);
        revoked.authorization.as_mut().unwrap().status = DeviceAuthorizationStatus::Revoked;

        assert_eq!(
            revoked.readiness(false, None),
            LocalDeviceReadiness::Blocked {
                reason: "device_revoked".to_owned()
            }
        );
        assert_eq!(
            revoked.readiness(true, Some("vault_context_mismatch")),
            LocalDeviceReadiness::Blocked {
                reason: "vault_context_mismatch".to_owned()
            }
        );
    }
}
