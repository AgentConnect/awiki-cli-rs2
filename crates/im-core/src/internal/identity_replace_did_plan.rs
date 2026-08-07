const REPLACE_DID_LOCAL_WRITES: &[&str] = &[
    "index.json",
    "identity.json",
    "auth.json",
    "did_document.json",
    "key-1-private.pem",
    "key-1-public.pem",
    "e2ee-signing-private.pem",
    "e2ee-agreement-private.pem",
    ".legacy-backup/replace-did",
    "sqlite.identity_did_history",
    "sqlite.owner_did_snapshot_refresh",
];

#[cfg(feature = "sqlite")]
const REPLACE_DID_REBIND_TABLES: &[&str] = &[
    "messages",
    "contacts",
    "contact_handle_bindings",
    "relationship_events",
    "groups",
    "group_members",
];

#[cfg(feature = "sqlite")]
const REPLACE_DID_E2EE_TABLES: &[&str] = &["e2ee_outbox", "e2ee_sessions"];

pub(crate) fn plan_replace_did_for_client(
    client: &crate::core::ImClient,
    mut request: crate::identity::ReplaceDidPlanRequest,
) -> crate::ImResult<crate::identity::ReplaceDidPlan> {
    validate_plan_request(&request)?;
    request.affected_local_state = plan_replace_did_local_state_rebind(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
        request.identity.did.as_str(),
        request.planned_new_did.as_str(),
    )?;
    plan_replace_did(request)
}

pub(crate) fn plan_replace_did(
    request: crate::identity::ReplaceDidPlanRequest,
) -> crate::ImResult<crate::identity::ReplaceDidPlan> {
    validate_plan_request(&request)?;
    let call = crate::internal::identity_wire::replace_did::build_replace_did_rpc_call(
        crate::internal::identity_wire::ReplaceDidRpcParams {
            new_did_document: serde_json::json!("generated_e1_document"),
            is_public: request.is_public,
            is_agent: request.is_agent,
            role: request.role,
            endpoint_url: request.endpoint_url,
        },
    );
    Ok(crate::identity::ReplaceDidPlan {
        action: "replace_did".to_string(),
        identity: request.identity.clone(),
        dangerous: true,
        risk_summary: vec![
            "Replaces the selected handle identity DID and local key material.".to_string(),
            "Requires a local backup before any execution path may continue.".to_string(),
            "Local SQLite DID history and owner_did snapshots are refreshed under the same owner identity."
                .to_string(),
        ],
        backup_plan: crate::identity::ReplaceDidBackupPlan {
            required: true,
            backup_path_preview: request.backup_path_preview.clone(),
            manifest_preview: crate::identity::ReplaceDidBackupManifestPreview {
                reason: "replace_did".to_string(),
                identity_name: identity_name(&request.identity),
                linked_identity_names: request.linked_identity_names.clone(),
                old_did: request.identity.did.clone(),
                old_dir_name: request.old_dir_name,
                planned_new_did: request.planned_new_did.clone(),
            },
        },
        local_rebind_plan: crate::identity::ReplaceDidLocalRebindPlan {
            required: request.identity.did != request.planned_new_did,
            old_owner_did: request.identity.did.clone(),
            new_owner_did: request.planned_new_did.clone(),
            destructive: false,
            dry_run_only: true,
        },
        affected_local_state: request.affected_local_state,
        remote_replace_did_call_preview: crate::identity::ReplaceDidRemoteCallPreview {
            endpoint: call.endpoint.to_string(),
            method: call.method.to_string(),
            params: call.params,
        },
        rollback_notes: vec![
            "Do not execute remote replace_did unless the backup manifest has been written."
                .to_string(),
            "If later execution fails after the remote call, restore the identity backup and inspect local DID history/snapshot counts."
                .to_string(),
        ],
        local_writes: REPLACE_DID_LOCAL_WRITES
            .iter()
            .map(|item| (*item).to_string())
            .collect(),
        warnings: Vec::new(),
    })
}

#[cfg(feature = "sqlite")]
pub(crate) fn plan_replace_did_local_state_rebind(
    sqlite_path: &std::path::Path,
    old_owner_did: &str,
    new_owner_did: &str,
) -> crate::ImResult<crate::identity::ReplaceDidAffectedLocalState> {
    let _ = (sqlite_path, old_owner_did, new_owner_did);
    // v17 keeps business rows owned by stable owner_identity_id. Replacing a DID
    // only records DID history and refreshes owner_did snapshots, so runtime
    // planning must not scan or rebind rows by owner_did.
    Ok(crate::identity::ReplaceDidAffectedLocalState {
        store_rebind_counts: zero_counts(REPLACE_DID_REBIND_TABLES),
        e2ee_cleanup_counts: zero_counts(REPLACE_DID_E2EE_TABLES),
    })
}

#[cfg(not(feature = "sqlite"))]
pub(crate) fn plan_replace_did_local_state_rebind(
    _sqlite_path: &std::path::Path,
    _old_owner_did: &str,
    _new_owner_did: &str,
) -> crate::ImResult<crate::identity::ReplaceDidAffectedLocalState> {
    Ok(crate::identity::ReplaceDidAffectedLocalState::default())
}

#[cfg(feature = "sqlite")]
fn zero_counts(tables: &[&str]) -> std::collections::BTreeMap<String, i64> {
    tables
        .iter()
        .map(|table| ((*table).to_string(), 0))
        .collect()
}

pub(crate) fn validate_plan_request(
    request: &crate::identity::ReplaceDidPlanRequest,
) -> crate::ImResult<()> {
    if request.identity.did.as_str().trim().is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("identity.did".to_string()),
            "identity DID is required",
        ));
    }
    if request.planned_new_did.as_str().trim().is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("planned_new_did".to_string()),
            "planned new DID is required",
        ));
    }
    if request.backup_path_preview.trim().is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("backup_path_preview".to_string()),
            "backup path preview is required",
        ));
    }
    Ok(())
}

pub(crate) fn validate_request_shape(
    plan: &crate::identity::ReplaceDidPlan,
) -> crate::ImResult<()> {
    if plan.action != "replace_did" {
        return Err(crate::ImError::invalid_input(
            Some("plan.action".to_string()),
            "replace DID execution requires a replace_did plan",
        ));
    }
    if !plan.dangerous {
        return Err(crate::ImError::invalid_input(
            Some("plan.dangerous".to_string()),
            "replace DID execution requires a dangerous plan",
        ));
    }
    if plan.backup_plan.backup_path_preview.trim().is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("plan.backup_plan.backup_path_preview".to_string()),
            "backup path preview is required",
        ));
    }
    if plan.identity.did == plan.local_rebind_plan.new_owner_did {
        return Err(crate::ImError::invalid_input(
            Some("plan.local_rebind_plan.new_owner_did".to_string()),
            "replacement DID must differ from the current DID",
        ));
    }
    Ok(())
}

fn identity_name(identity: &crate::identity::IdentitySummary) -> String {
    identity
        .local_alias
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| identity.id.as_str().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn replace_did_plan_contains_backup_remote_rebind_and_counts() {
        let mut store_counts = BTreeMap::new();
        store_counts.insert("messages".to_string(), 2);
        let mut e2ee_counts = BTreeMap::new();
        e2ee_counts.insert("e2ee_sessions".to_string(), 1);

        let plan = plan_replace_did(crate::identity::ReplaceDidPlanRequest {
            identity: identity_summary(),
            linked_identity_names: vec!["alice".to_string(), "alias".to_string()],
            planned_new_did: crate::ids::Did::parse("did:wba:awiki.test:alice:e1_new").unwrap(),
            backup_path_preview: "/tmp/.legacy-backup/replace-did/<timestamp>-alice".to_string(),
            old_dir_name: "e1_old".to_string(),
            is_public: Some(false),
            is_agent: None,
            role: Some(String::new()),
            endpoint_url: Some("https://example.test/agent".to_string()),
            affected_local_state: crate::identity::ReplaceDidAffectedLocalState {
                store_rebind_counts: store_counts,
                e2ee_cleanup_counts: e2ee_counts,
            },
        })
        .unwrap();

        assert_eq!(plan.action, "replace_did");
        assert!(plan.dangerous);
        assert!(plan.backup_plan.required);
        assert_eq!(plan.backup_plan.manifest_preview.reason, "replace_did");
        assert_eq!(plan.remote_replace_did_call_preview.method, "replace_did");
        assert_eq!(
            plan.remote_replace_did_call_preview.params["is_public"],
            false
        );
        assert_eq!(
            plan.remote_replace_did_call_preview.params["role"],
            serde_json::Value::Null
        );
        assert_eq!(
            plan.remote_replace_did_call_preview.params["endpoint_url"],
            "https://example.test/agent"
        );
        assert!(plan.local_rebind_plan.dry_run_only);
        assert_eq!(plan.affected_local_state.store_rebind_counts["messages"], 2);
        assert_eq!(
            plan.affected_local_state.e2ee_cleanup_counts["e2ee_sessions"],
            1
        );
        assert!(plan
            .local_writes
            .contains(&".legacy-backup/replace-did".to_string()));
        assert!(plan
            .rollback_notes
            .iter()
            .any(|note| note.contains("backup manifest")));
    }

    #[cfg(feature = "sqlite")]
    #[test]
    fn replace_did_service_does_not_count_business_rows_by_owner_did() {
        let root = tempfile::TempDir::new().unwrap();
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
                    identity_root_dir: root.path().join("identities"),
                    registry_path: root.path().join("identities").join("registry.json"),
                    default_identity_path: Some(root.path().join("identities").join("default")),
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
        let client = core
            .client(crate::identity::IdentitySelector::Did(
                crate::ids::Did::parse("did:wba:awiki.test:alice:e1_old").unwrap(),
            ))
            .unwrap();
        let sqlite_path = &client.core_inner().sdk_paths().local_state.sqlite_path;
        let connection = crate::internal::local_state::open_writable(sqlite_path).unwrap();
        connection
            .execute(
                "INSERT INTO messages (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, stored_at)
                 VALUES (?1, ?2, ?3, ?4, ?4, ?5)",
                rusqlite::params![
                    "replace-did-msg-1",
                    "alice-id",
                    "did:wba:awiki.test:alice:e1_old",
                    "dm:did:example:bob",
                    "2026-05-25T00:00:00Z",
                ],
            )
            .unwrap();
        drop(connection);

        let mut request = crate::identity::ReplaceDidPlanRequest {
            identity: identity_summary(),
            linked_identity_names: vec!["alice".to_string()],
            planned_new_did: crate::ids::Did::parse("did:wba:awiki.test:alice:e1_new").unwrap(),
            backup_path_preview: "/tmp/.legacy-backup/replace-did/<timestamp>-alice".to_string(),
            old_dir_name: "e1_old".to_string(),
            is_public: Some(false),
            is_agent: None,
            role: None,
            endpoint_url: Some("https://example.test/agent".to_string()),
            affected_local_state: crate::identity::ReplaceDidAffectedLocalState::default(),
        };
        let plan = client.identity().replace_did_plan(request.clone()).unwrap();
        assert_eq!(plan.affected_local_state.store_rebind_counts["messages"], 0);
        assert_eq!(
            plan.affected_local_state.e2ee_cleanup_counts["e2ee_sessions"],
            0
        );

        request.planned_new_did = request.identity.did.clone();
        let same_did_plan = client.identity().replace_did_plan(request).unwrap();
        assert_eq!(
            same_did_plan.affected_local_state.store_rebind_counts["messages"],
            0
        );
        assert_eq!(
            same_did_plan.affected_local_state.e2ee_cleanup_counts["e2ee_sessions"],
            0
        );
    }

    fn identity_summary() -> crate::identity::IdentitySummary {
        crate::identity::IdentitySummary {
            id: crate::ids::IdentityId::parse("alice-id").unwrap(),
            did: crate::ids::Did::parse("did:wba:awiki.test:alice:e1_old").unwrap(),
            handle: Some(crate::ids::Handle::parse("alice.awiki.test", "").unwrap()),
            display_name: Some("Alice".to_string()),
            local_alias: Some("alice".to_string()),
            device_id: None,
            is_default: true,
            readiness: crate::identity::IdentityReadiness {
                ready_for_auth: true,
                ready_for_messaging: true,
                missing: Vec::new(),
            },
        }
    }
}
