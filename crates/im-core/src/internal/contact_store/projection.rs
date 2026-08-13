use serde_json::Value;

use super::records::ContactRecord;
use crate::internal::local_state::owner_scope::OwnerScope;

pub(crate) fn record_from_save_request(
    client: &crate::core::ImClient,
    request: &crate::directory::SaveContactRequest,
    did: crate::ids::Did,
) -> crate::ImResult<ContactRecord> {
    let scope = OwnerScope::for_client(client)?;
    Ok(ContactRecord {
        owner_identity_id: scope.owner_identity_id.clone(),
        owner_did: scope.owner_did.clone(),
        did: did.as_str().to_string(),
        name: request.display_name.clone().unwrap_or_default(),
        handle: request
            .handle
            .as_ref()
            .map(|handle| handle.as_str().to_string())
            .unwrap_or_else(|| {
                if request.peer.as_str().starts_with("did:") {
                    String::new()
                } else {
                    request.peer.as_str().to_string()
                }
            }),
        relationship: request.relationship.clone().unwrap_or_default(),
        note: request.note.clone().unwrap_or_default(),
        source_type: "directory.save_contact".to_string(),
        credential_name: credential_name(&scope),
        ..ContactRecord::default()
    })
}

pub(crate) fn record_from_profile(
    client: &crate::core::ImClient,
    profile: &crate::identity::Profile,
    source_type: &str,
) -> crate::ImResult<ContactRecord> {
    let scope = OwnerScope::for_client(client)?;
    Ok(ContactRecord {
        owner_identity_id: scope.owner_identity_id.clone(),
        owner_did: scope.owner_did.clone(),
        did: profile.subject.as_str().to_string(),
        name: profile.display_name.clone().unwrap_or_default(),
        handle: profile
            .handle
            .as_ref()
            .map(|handle| handle.as_str().to_string())
            .unwrap_or_default(),
        bio: profile
            .bio
            .clone()
            .or_else(|| profile.description.clone())
            .unwrap_or_default(),
        profile_md: profile.markdown.clone().unwrap_or_default(),
        tags: profile.tags.join(","),
        source_type: source_type.to_string(),
        last_seen_at: profile.updated_at.clone().unwrap_or_default(),
        metadata: metadata_json(profile),
        credential_name: credential_name(&scope),
        ..ContactRecord::default()
    })
}

#[cfg(any(feature = "blocking", test))]
pub(crate) fn project_directory_resolution(
    client: &crate::core::ImClient,
    resolution: &crate::directory::DirectoryResolution,
) {
    let mut connection = match super::open_writable(client) {
        Ok(connection) => connection,
        Err(_) => return,
    };
    let record = match record_from_directory_resolution(client, resolution) {
        Ok(record) => record,
        Err(_) => return,
    };
    if super::records::upsert_contact(&mut connection, record).is_err() {
        return;
    }
    if let Some(profile) = resolution.profile.as_ref() {
        let Ok(scope) = OwnerScope::for_client(client) else {
            return;
        };
        let _ = crate::internal::local_state::peer_profiles::refresh_existing_from_public_profile(
            &connection,
            &scope.owner_identity_id,
            &resolution.did,
            profile,
        );
    }
}

#[cfg(not(any(feature = "blocking", test)))]
pub(crate) fn project_directory_resolution(
    _client: &crate::core::ImClient,
    _resolution: &crate::directory::DirectoryResolution,
) {
}

pub(crate) async fn project_directory_resolution_async(
    client: &crate::core::ImClient,
    resolution: &crate::directory::DirectoryResolution,
) -> crate::ImResult<()> {
    let record = record_from_directory_resolution(client, resolution)?;
    let db = client.core_inner().local_state_db().await?;
    db.upsert_contact(record).await?;
    if let Some(profile) = resolution.profile.as_ref() {
        let scope = OwnerScope::for_client(client)?;
        db.refresh_existing_persona_profile(
            scope.owner_identity_id,
            resolution.did.clone(),
            profile.clone(),
        )
        .await?;
    }
    Ok(())
}

fn record_from_directory_resolution(
    client: &crate::core::ImClient,
    resolution: &crate::directory::DirectoryResolution,
) -> crate::ImResult<ContactRecord> {
    let scope = OwnerScope::for_client(client)?;
    let credential_name = credential_name(&scope);
    let record = resolution
        .profile
        .as_ref()
        .map(|profile| record_from_profile(client, profile, "directory.profile_projection"))
        .transpose()?
        .unwrap_or_else(|| ContactRecord {
            owner_identity_id: scope.owner_identity_id,
            owner_did: scope.owner_did,
            did: resolution.did.as_str().to_string(),
            handle: resolution
                .handle
                .as_ref()
                .map(|handle| handle.as_str().to_string())
                .unwrap_or_default(),
            source_type: "directory.resolve_peer".to_string(),
            credential_name,
            ..ContactRecord::default()
        });
    Ok(record)
}

fn metadata_json(profile: &crate::identity::Profile) -> String {
    let mut object: serde_json::Map<String, Value> = profile
        .metadata
        .iter()
        .map(|attribute| {
            (
                attribute.key.clone(),
                Value::String(attribute.value.clone()),
            )
        })
        .collect();
    if let Some(avatar_uri) = profile.effective_avatar_uri() {
        object.insert("avatar_uri".to_string(), Value::String(avatar_uri.clone()));
        object
            .entry("avatar_url".to_string())
            .or_insert_with(|| Value::String(avatar_uri.clone()));
    }
    if let Some(profile_uri) = profile.profile_uri.as_ref() {
        object.insert(
            "profile_uri".to_string(),
            Value::String(profile_uri.clone()),
        );
    }
    if let Some(subject_type) = profile.subject_type.as_ref() {
        object.insert(
            "subject_type".to_string(),
            Value::String(subject_type.clone()),
        );
    }
    if let Some(version_id) = profile.version_id.as_ref() {
        object.insert("versionId".to_string(), Value::String(version_id.clone()));
    }
    if let Some(ttl) = profile.ttl {
        object.insert("ttl".to_string(), serde_json::json!(ttl));
    }
    if object.is_empty() {
        return String::new();
    }
    let value = Value::Object(object);
    value.to_string()
}

fn credential_name(scope: &OwnerScope) -> String {
    scope
        .credential_name
        .clone()
        .unwrap_or_else(|| scope.owner_identity_id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contacts_profile_projection_maps_profile_to_contact_record() {
        let client = fixture_client();
        let mut profile =
            crate::identity::Profile::new(crate::ids::Did::parse("did:example:bob").unwrap());
        profile.handle = Some(crate::ids::Handle::parse("bob.awiki.test", "").unwrap());
        profile.display_name = Some("Bob".to_string());
        profile.bio = Some("Builder".to_string());
        profile.tags = vec!["rust".to_string(), "sdk".to_string()];
        profile.markdown = Some("## Bob".to_string());
        profile.avatar_uri = Some("https://cdn.test/bob.png".to_string());
        profile.profile_uri = Some("https://bob.awiki.test/".to_string());
        profile.subject_type = Some("person".to_string());
        profile.updated_at = Some("2026-05-21T00:00:00Z".to_string());
        profile.metadata = vec![crate::identity::ProfileAttribute {
            key: "source".to_string(),
            value: "profile".to_string(),
        }];

        let record = record_from_profile(&client, &profile, "directory.profile_projection")
            .expect("profile projection record");

        assert_eq!(record.owner_identity_id, "alice-id");
        assert_eq!(record.owner_did, "did:example:alice");
        assert_eq!(record.did, "did:example:bob");
        assert_eq!(record.handle, "bob.awiki.test");
        assert_eq!(record.name, "Bob");
        assert_eq!(record.tags, "rust,sdk");
        assert_eq!(record.profile_md, "## Bob");
        assert_eq!(record.last_seen_at, "2026-05-21T00:00:00Z");
        let metadata: serde_json::Value = serde_json::from_str(&record.metadata).unwrap();
        assert_eq!(metadata["source"], "profile");
        assert_eq!(metadata["avatar_uri"], "https://cdn.test/bob.png");
        assert_eq!(metadata["avatar_url"], "https://cdn.test/bob.png");
        assert_eq!(metadata["profile_uri"], "https://bob.awiki.test/");
        assert_eq!(metadata["subject_type"], "person");
        assert_eq!(record.credential_name, "alice");
    }

    fn fixture_client() -> crate::core::ImClient {
        let root = tempfile::TempDir::new().unwrap();
        let identities = root.path().join("identities");
        std::fs::create_dir_all(identities.join("alice")).unwrap();
        std::fs::write(identities.join("default"), "alice\n").unwrap();
        std::fs::write(
            identities.join("registry.json"),
            r#"{
              "default_identity": "alice",
              "identities": [{
                "id": "alice-id",
                "did": "did:example:alice",
                "handle": "alice.awiki.test",
                "display_name": "Alice",
                "local_alias": "alice",
                "ready_for_auth": true,
                "ready_for_messaging": true,
                "missing": []
              }]
            }"#,
        )
        .unwrap();
        std::fs::write(
            identities.join("alice").join("did.json"),
            r#"{"id":"did:example:alice","controller":"did:example:alice"}"#,
        )
        .unwrap();
        std::fs::write(identities.join("alice").join("private.key"), "key\n").unwrap();
        std::fs::write(
            identities.join("alice").join("auth.json"),
            r#"{"jwt_token":"token"}"#,
        )
        .unwrap();
        let core = crate::core::ImCore::new(
            crate::config::ImCoreConfig {
                service_base_url: crate::config::ServiceEndpoint::parse("https://example.test")
                    .unwrap(),
                did_domain: "awiki.test".to_string(),
                client_version_info: None,
                user_service_endpoint: None,
                message_service_endpoint: None,
                mail_service_endpoint: None,
                anp_service_endpoint: None,
                anp_service_did: None,
                ca_bundle: None,
                transport_policy: crate::config::MessageTransportPolicy::HttpOnly,
            },
            crate::paths::ImCorePaths {
                identities: crate::paths::IdentityRegistryPaths {
                    identity_root_dir: identities.clone(),
                    registry_path: identities.join("registry.json"),
                    default_identity_path: Some(identities.join("default")),
                },
                local_state: crate::paths::LocalStatePaths {
                    sqlite_path: root.path().join("local").join("im.sqlite"),
                },
                runtime: crate::paths::RuntimePaths {
                    cache_dir: root.path().join("cache"),
                    temp_dir: root.path().join("tmp"),
                },
            },
        )
        .unwrap();
        core.client(crate::identity::IdentitySelector::LocalAlias(
            "alice".to_string(),
        ))
        .unwrap()
    }
}
