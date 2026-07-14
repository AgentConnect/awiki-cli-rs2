//! Canonical authority and Persona identity derived only from verified Handle data.

use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PeerPersona {
    pub(crate) peer_persona_id: String,
    pub(crate) authority_namespace: String,
    pub(crate) authority_subject_id: String,
    pub(crate) full_handle: String,
}

impl PeerPersona {
    pub(crate) fn from_verified_handle(
        authority_domain: &str,
        authority_subject_id: &str,
        full_handle: &str,
        status: Option<&str>,
    ) -> crate::ImResult<Self> {
        let authority_namespace = normalize_authority_namespace(authority_domain)?;
        let authority_subject_id = required("authority_subject_id", authority_subject_id)?;
        validate_authority_subject_id(&authority_subject_id)?;
        let full_handle = normalize_full_handle(full_handle)?;
        validate_handle_authority(&full_handle, &authority_namespace)?;
        validate_available_status(status)?;

        let peer_persona_id = format!(
            "persona:v1:{}",
            sha256_hex(
                format!(
                    "peer-persona:v1\nauthority:{authority_namespace}\nsubject:{authority_subject_id}\nhandle:{full_handle}"
                )
                .as_bytes()
            )
        );
        Ok(Self {
            peer_persona_id,
            authority_namespace,
            authority_subject_id,
            full_handle,
        })
    }

    pub(crate) fn direct_conversation_id(&self) -> String {
        crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(
            &crate::internal::local_state::owner_scope::DirectPeerScope {
                user_id: self.authority_subject_id.clone(),
                full_handle: self.full_handle.clone(),
            },
        )
    }
}

fn validate_authority_subject_id(value: &str) -> crate::ImResult<()> {
    if value.to_ascii_lowercase().starts_with("did:") {
        return Err(crate::ImError::IdentityUnresolved {
            detail: "Handle authority subject must not fall back to a credential DID".to_owned(),
        });
    }
    Ok(())
}

pub(crate) fn normalize_authority_namespace(value: &str) -> crate::ImResult<String> {
    let trimmed = value.trim().trim_end_matches('.');
    if trimmed.is_empty()
        || trimmed.contains("://")
        || trimmed.contains('/')
        || trimmed.contains('@')
        || trimmed.contains(':')
    {
        return Err(crate::ImError::IdentityUnresolved {
            detail: "authoritative Handle domain is missing or is not a bare host".to_owned(),
        });
    }
    let ascii =
        idna::domain_to_ascii_strict(trimmed).map_err(|_| crate::ImError::IdentityUnresolved {
            detail: "authoritative Handle domain is not a valid IDNA host".to_owned(),
        })?;
    let normalized = ascii.to_ascii_lowercase();
    if normalized.is_empty() || normalized.split('.').any(|label| label.is_empty()) {
        return Err(crate::ImError::IdentityUnresolved {
            detail: "authoritative Handle domain is empty after normalization".to_owned(),
        });
    }
    Ok(normalized)
}

fn normalize_full_handle(value: &str) -> crate::ImResult<String> {
    let value = value.trim().trim_start_matches('@').trim_end_matches('.');
    if value.is_empty() || value.contains("://") || value.contains('/') || value.contains('@') {
        return Err(crate::ImError::IdentityUnresolved {
            detail: "verified full Handle is missing or malformed".to_owned(),
        });
    }
    let normalized = idna::domain_to_ascii_strict(value)
        .map_err(|_| crate::ImError::IdentityUnresolved {
            detail: "verified full Handle is not valid IDNA".to_owned(),
        })?
        .to_ascii_lowercase();
    Ok(normalized)
}

fn validate_handle_authority(full_handle: &str, authority_namespace: &str) -> crate::ImResult<()> {
    let Some((subject, domain)) = full_handle.split_once('.') else {
        return Err(crate::ImError::IdentityUnresolved {
            detail: "verified full Handle has no authority domain".to_owned(),
        });
    };
    if subject.is_empty() || domain != authority_namespace {
        return Err(crate::ImError::IdentityBindingConflict {
            detail: "verified Handle domain does not match its authority namespace".to_owned(),
        });
    }
    Ok(())
}

fn validate_available_status(status: Option<&str>) -> crate::ImResult<()> {
    let status = status.unwrap_or_default().trim().to_ascii_lowercase();
    if !matches!(status.as_str(), "active" | "bound" | "verified") {
        return Err(crate::ImError::IdentityUnresolved {
            detail: "Handle authority did not return an available binding status".to_owned(),
        });
    }
    Ok(())
}

fn required(field: &'static str, value: &str) -> crate::ImResult<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(crate::ImError::IdentityUnresolved {
            detail: format!("{field} is required for a canonical Persona"),
        });
    }
    Ok(value.to_owned())
}

fn sha256_hex(input: &[u8]) -> String {
    let digest = Sha256::digest(input);
    let mut value = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(value, "{byte:02x}");
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authority_namespace_is_stable_across_case_root_dot_and_idna() {
        assert_eq!(
            normalize_authority_namespace(" AWIKI.INFO. ").unwrap(),
            "awiki.info"
        );
        assert_eq!(
            normalize_authority_namespace("BÜCHER.example").unwrap(),
            normalize_authority_namespace("xn--bcher-kva.example").unwrap()
        );
    }

    #[test]
    fn authority_namespace_rejects_url_port_and_display_input() {
        for invalid in [
            "https://awiki.info",
            "awiki.info/path",
            "user@awiki.info",
            "awiki.info:443",
        ] {
            assert!(normalize_authority_namespace(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn persona_is_stable_across_binding_generation_and_did_rotation() {
        let first = PeerPersona::from_verified_handle(
            "AWIKI.INFO.",
            "user-alice",
            "Alice.AWiki.Info",
            Some("active"),
        )
        .unwrap();
        let rotated = PeerPersona::from_verified_handle(
            "awiki.info",
            "user-alice",
            "alice.awiki.info",
            Some("verified"),
        )
        .unwrap();
        assert_eq!(first, rotated);
        assert_eq!(
            first.direct_conversation_id(),
            rotated.direct_conversation_id()
        );
    }

    #[test]
    fn same_subject_in_different_authorities_is_not_the_same_persona() {
        let first = PeerPersona::from_verified_handle(
            "awiki.info",
            "user-alice",
            "alice.awiki.info",
            Some("active"),
        )
        .unwrap();
        let second = PeerPersona::from_verified_handle(
            "anpclaw.com",
            "user-alice",
            "alice.anpclaw.com",
            Some("active"),
        )
        .unwrap();
        assert_ne!(first.peer_persona_id, second.peer_persona_id);
        assert_ne!(
            first.direct_conversation_id(),
            second.direct_conversation_id()
        );
    }

    #[test]
    fn missing_subject_or_unavailable_status_does_not_create_persona() {
        assert!(PeerPersona::from_verified_handle(
            "awiki.info",
            "",
            "alice.awiki.info",
            Some("active")
        )
        .is_err());
        assert!(PeerPersona::from_verified_handle(
            "awiki.info",
            "user-alice",
            "alice.awiki.info",
            None
        )
        .is_err());
        assert!(PeerPersona::from_verified_handle(
            "awiki.info",
            "did:wba:awiki.info:alice:e1_fallback",
            "alice.awiki.info",
            Some("active")
        )
        .is_err());
    }
}
