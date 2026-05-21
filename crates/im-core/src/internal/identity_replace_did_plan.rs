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
    "sqlite.owner_did_rebind",
    "sqlite.e2ee_cleanup",
];

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
            "Local SQLite owner state must be rebound from the old DID to the planned DID."
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
            destructive: true,
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
            "If later execution fails after the remote call, restore the identity backup and inspect local owner rebind counts."
                .to_string(),
        ],
        local_writes: REPLACE_DID_LOCAL_WRITES
            .iter()
            .map(|item| (*item).to_string())
            .collect(),
        warnings: Vec::new(),
    })
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
