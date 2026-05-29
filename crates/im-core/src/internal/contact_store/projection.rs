use serde_json::Value;

use super::records::ContactRecord;

pub(crate) fn record_from_save_request(
    client: &crate::core::ImClient,
    request: &crate::directory::SaveContactRequest,
    did: crate::ids::Did,
) -> ContactRecord {
    ContactRecord {
        owner_identity_id: client.current_identity().id.as_str().to_string(),
        owner_did: client.did().as_str().to_string(),
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
        credential_name: client.current_identity().id.as_str().to_string(),
        ..ContactRecord::default()
    }
}

pub(crate) fn record_from_profile(
    client: &crate::core::ImClient,
    profile: &crate::identity::Profile,
    source_type: &str,
) -> ContactRecord {
    ContactRecord {
        owner_identity_id: client.current_identity().id.as_str().to_string(),
        owner_did: client.did().as_str().to_string(),
        did: profile.subject.as_str().to_string(),
        name: profile.display_name.clone().unwrap_or_default(),
        handle: profile
            .handle
            .as_ref()
            .map(|handle| handle.as_str().to_string())
            .unwrap_or_default(),
        bio: profile.bio.clone().unwrap_or_default(),
        profile_md: profile.markdown.clone().unwrap_or_default(),
        tags: profile.tags.join(","),
        source_type: source_type.to_string(),
        last_seen_at: profile.updated_at.clone().unwrap_or_default(),
        metadata: metadata_json(&profile.metadata),
        credential_name: client.current_identity().id.as_str().to_string(),
        ..ContactRecord::default()
    }
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
    let record = resolution
        .profile
        .as_ref()
        .map(|profile| record_from_profile(client, profile, "directory.profile_projection"))
        .unwrap_or_else(|| ContactRecord {
            owner_identity_id: client.current_identity().id.as_str().to_string(),
            owner_did: client.did().as_str().to_string(),
            did: resolution.did.as_str().to_string(),
            handle: resolution
                .handle
                .as_ref()
                .map(|handle| handle.as_str().to_string())
                .unwrap_or_default(),
            source_type: "directory.resolve_peer".to_string(),
            credential_name: client.current_identity().id.as_str().to_string(),
            ..ContactRecord::default()
        });
    let _ = super::records::upsert_contact(&mut connection, record);
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
    let record = record_from_directory_resolution(client, resolution);
    let db = client.core_inner().local_state_db().await?;
    db.upsert_contact(record).await
}

fn record_from_directory_resolution(
    client: &crate::core::ImClient,
    resolution: &crate::directory::DirectoryResolution,
) -> ContactRecord {
    resolution
        .profile
        .as_ref()
        .map(|profile| record_from_profile(client, profile, "directory.profile_projection"))
        .unwrap_or_else(|| ContactRecord {
            owner_identity_id: client.current_identity().id.as_str().to_string(),
            owner_did: client.did().as_str().to_string(),
            did: resolution.did.as_str().to_string(),
            handle: resolution
                .handle
                .as_ref()
                .map(|handle| handle.as_str().to_string())
                .unwrap_or_default(),
            source_type: "directory.resolve_peer".to_string(),
            credential_name: client.current_identity().id.as_str().to_string(),
            ..ContactRecord::default()
        })
}

fn metadata_json(metadata: &[crate::identity::ProfileAttribute]) -> String {
    if metadata.is_empty() {
        return String::new();
    }
    let value = Value::Object(
        metadata
            .iter()
            .map(|attribute| {
                (
                    attribute.key.clone(),
                    Value::String(attribute.value.clone()),
                )
            })
            .collect(),
    );
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contacts_profile_projection_maps_profile_to_contact_record() {
        let client = fixture_client();
        let profile = crate::identity::Profile {
            subject: crate::ids::Did::parse("did:example:bob").unwrap(),
            handle: Some(crate::ids::Handle::parse("bob.awiki.test", "").unwrap()),
            display_name: Some("Bob".to_string()),
            bio: Some("Builder".to_string()),
            tags: vec!["rust".to_string(), "sdk".to_string()],
            markdown: Some("## Bob".to_string()),
            avatar_url: None,
            updated_at: Some("2026-05-21T00:00:00Z".to_string()),
            metadata: vec![crate::identity::ProfileAttribute {
                key: "source".to_string(),
                value: "profile".to_string(),
            }],
        };

        let record = record_from_profile(&client, &profile, "directory.profile_projection");

        assert_eq!(record.owner_did, "did:example:alice");
        assert_eq!(record.did, "did:example:bob");
        assert_eq!(record.handle, "bob.awiki.test");
        assert_eq!(record.name, "Bob");
        assert_eq!(record.tags, "rust,sdk");
        assert_eq!(record.profile_md, "## Bob");
        assert_eq!(record.last_seen_at, "2026-05-21T00:00:00Z");
        assert_eq!(record.metadata, r#"{"source":"profile"}"#);
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
