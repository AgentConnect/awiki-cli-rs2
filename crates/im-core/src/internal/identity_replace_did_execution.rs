use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReplaceDidBackupResult {
    pub(crate) backup_path: String,
    pub(crate) manifest: crate::identity::ReplaceDidBackupManifestPreview,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReplaceDidLocalUpdate {
    pub(crate) identity: crate::identity::IdentitySummary,
    pub(crate) new_did: crate::ids::Did,
}

pub(crate) trait ReplaceDidExecutionBridge {
    fn create_backup(
        &mut self,
        plan: &crate::identity::ReplaceDidPlan,
    ) -> crate::ImResult<ReplaceDidBackupResult>;

    fn remote_replace_did(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value>;

    fn replace_local_identity(
        &mut self,
        request: &crate::identity::ReplaceDidExecutionRequest,
        remote_result: &Value,
    ) -> crate::ImResult<ReplaceDidLocalUpdate>;

    fn rebind_local_state(
        &mut self,
        owner_identity_id: &crate::ids::IdentityId,
        old_did: &crate::ids::Did,
        new_did: &crate::ids::Did,
    ) -> crate::ImResult<crate::identity::ReplaceDidAffectedLocalState>;
}

pub(crate) struct ReplaceDidExecutionRuntime<'a, B> {
    _client: &'a crate::core::ImClient,
    bridge: B,
}

impl<'a, B> ReplaceDidExecutionRuntime<'a, B>
where
    B: ReplaceDidExecutionBridge,
{
    pub(crate) fn new(client: &'a crate::core::ImClient, bridge: B) -> Self {
        Self {
            _client: client,
            bridge,
        }
    }

    pub(crate) fn execute(
        mut self,
        request: crate::identity::ReplaceDidExecutionRequest,
    ) -> crate::ImResult<crate::identity::ReplaceDidExecutionResult> {
        validate_request(&request)?;
        let backup = self.bridge.create_backup(&request.plan)?;
        let call = crate::internal::identity_wire::replace_did::build_replace_did_rpc_call(
            crate::internal::identity_wire::ReplaceDidRpcParams {
                new_did_document: request.generated_identity.did_document.clone(),
                is_public: request.is_public,
                is_agent: request.is_agent,
                role: request.role.clone(),
                endpoint_url: request.endpoint_url.clone(),
            },
        );
        let remote_result =
            self.bridge
                .remote_replace_did(call.endpoint, call.method, call.params.clone())?;
        let local_update = self
            .bridge
            .replace_local_identity(&request, &remote_result)
            .map_err(|err| local_failure_after_remote("local identity update", err, &backup))?;
        let affected_local_state = self
            .bridge
            .rebind_local_state(
                &request.plan.identity.id,
                &request.plan.identity.did,
                &local_update.new_did,
            )
            .map_err(|err| local_failure_after_remote("local DID history update", err, &backup))?;

        Ok(crate::identity::ReplaceDidExecutionResult {
            identity: local_update.identity,
            old_did: request.plan.identity.did,
            new_did: local_update.new_did,
            backup_path: backup.backup_path,
            backup_manifest: backup.manifest,
            affected_local_state,
            remote_result,
            warnings: Vec::new(),
            recovery_notes: Vec::new(),
        })
    }
}

fn validate_request(request: &crate::identity::ReplaceDidExecutionRequest) -> crate::ImResult<()> {
    crate::internal::identity_replace_did_plan::validate_request_shape(&request.plan)?;
    if !request.plan.backup_plan.required {
        return Err(crate::ImError::invalid_input(
            Some("plan.backup_plan.required".to_string()),
            "replace DID execution requires a backup plan",
        ));
    }
    if request
        .plan
        .backup_plan
        .backup_path_preview
        .trim()
        .is_empty()
    {
        return Err(crate::ImError::invalid_input(
            Some("plan.backup_plan.backup_path_preview".to_string()),
            "replace DID execution requires a backup path preview",
        ));
    }
    if !request.generated_identity.did_document.is_object() {
        return Err(crate::ImError::invalid_input(
            Some("generated_identity.did_document".to_string()),
            "replacement DID document must be an object",
        ));
    }
    if request.generated_identity.unique_id.trim().is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("generated_identity.unique_id".to_string()),
            "replacement identity unique_id must not be empty",
        ));
    }
    Ok(())
}

fn local_failure_after_remote(
    phase: &str,
    err: crate::ImError,
    backup: &ReplaceDidBackupResult,
) -> crate::ImError {
    crate::ImError::LocalStateUnavailable {
        detail: format!(
            "{phase} failed after remote replace_did succeeded: {err}. Restore from backup {} or inspect backup manifest for manual recovery.",
            backup.backup_path
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_did_execution_orders_backup_remote_local_history_update() {
        let client = fixture_client();
        let result = ReplaceDidExecutionRuntime::new(
            &client,
            TestBridge {
                events: Vec::new(),
                fail_backup: false,
                fail_local: false,
                fail_rebind: false,
            },
        )
        .execute(execution_request())
        .unwrap();

        assert_eq!(result.backup_path, "/tmp/backup");
        assert_eq!(result.new_did.as_str(), "did:wba:awiki.test:alice:e1_new");
        assert_eq!(result.remote_result["user_id"], "user-alice");
        assert_eq!(
            result.affected_local_state.store_rebind_counts["messages"],
            1
        );
    }

    #[test]
    fn replace_did_execution_stops_before_remote_when_backup_fails() {
        let client = fixture_client();
        let err = ReplaceDidExecutionRuntime::new(
            &client,
            TestBridge {
                events: Vec::new(),
                fail_backup: true,
                fail_local: false,
                fail_rebind: false,
            },
        )
        .execute(execution_request())
        .unwrap_err();

        assert!(err.to_string().contains("backup failed"));
    }

    #[test]
    fn replace_did_execution_reports_manual_recovery_when_local_update_fails_after_remote() {
        let client = fixture_client();
        let err = ReplaceDidExecutionRuntime::new(
            &client,
            TestBridge {
                events: Vec::new(),
                fail_backup: false,
                fail_local: true,
                fail_rebind: false,
            },
        )
        .execute(execution_request())
        .unwrap_err();

        assert!(err
            .to_string()
            .contains("after remote replace_did succeeded"));
        assert!(err.to_string().contains("/tmp/backup"));
    }

    #[derive(Debug)]
    struct TestBridge {
        events: Vec<&'static str>,
        fail_backup: bool,
        fail_local: bool,
        fail_rebind: bool,
    }

    impl ReplaceDidExecutionBridge for TestBridge {
        fn create_backup(
            &mut self,
            plan: &crate::identity::ReplaceDidPlan,
        ) -> crate::ImResult<ReplaceDidBackupResult> {
            self.events.push("backup");
            if self.fail_backup {
                return Err(crate::ImError::Internal {
                    message: "backup failed".to_string(),
                });
            }
            Ok(ReplaceDidBackupResult {
                backup_path: "/tmp/backup".to_string(),
                manifest: plan.backup_plan.manifest_preview.clone(),
            })
        }

        fn remote_replace_did(
            &mut self,
            endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            self.events.push("remote");
            assert_eq!(
                endpoint,
                crate::internal::identity_wire::DID_AUTH_RPC_ENDPOINT
            );
            assert_eq!(method, "replace_did");
            assert!(params["new_did_document"].is_object());
            Ok(serde_json::json!({
                "did": "did:wba:awiki.test:alice:e1_new",
                "user_id": "user-alice",
                "access_token": "jwt-new"
            }))
        }

        fn replace_local_identity(
            &mut self,
            _request: &crate::identity::ReplaceDidExecutionRequest,
            _remote_result: &Value,
        ) -> crate::ImResult<ReplaceDidLocalUpdate> {
            self.events.push("local");
            if self.fail_local {
                return Err(crate::ImError::LocalStateUnavailable {
                    detail: "write identity failed".to_string(),
                });
            }
            Ok(ReplaceDidLocalUpdate {
                identity: identity_summary("did:wba:awiki.test:alice:e1_new"),
                new_did: crate::ids::Did::parse("did:wba:awiki.test:alice:e1_new").unwrap(),
            })
        }

        fn rebind_local_state(
            &mut self,
            _owner_identity_id: &crate::ids::IdentityId,
            _old_did: &crate::ids::Did,
            _new_did: &crate::ids::Did,
        ) -> crate::ImResult<crate::identity::ReplaceDidAffectedLocalState> {
            self.events.push("history");
            if self.fail_rebind {
                return Err(crate::ImError::LocalStateUnavailable {
                    detail: "history update failed".to_string(),
                });
            }
            Ok(crate::identity::ReplaceDidAffectedLocalState {
                store_rebind_counts: [("messages".to_string(), 1)].into_iter().collect(),
                e2ee_cleanup_counts: [("e2ee_sessions".to_string(), 1)].into_iter().collect(),
            })
        }
    }

    fn execution_request() -> crate::identity::ReplaceDidExecutionRequest {
        crate::identity::ReplaceDidExecutionRequest {
            plan: crate::internal::identity_replace_did_plan::plan_replace_did(
                crate::identity::ReplaceDidPlanRequest {
                    identity: identity_summary("did:wba:awiki.test:alice:e1_old"),
                    linked_identity_names: vec!["alice".to_string()],
                    planned_new_did: crate::ids::Did::parse("did:wba:awiki.test:alice:e1_new")
                        .unwrap(),
                    backup_path_preview: "/tmp/backup-preview".to_string(),
                    old_dir_name: "e1_old".to_string(),
                    is_public: Some(false),
                    is_agent: Some(true),
                    role: Some(String::new()),
                    endpoint_url: Some("https://example.test/agent".to_string()),
                    affected_local_state: crate::identity::ReplaceDidAffectedLocalState::default(),
                },
            )
            .unwrap(),
            generated_identity: crate::identity::ReplaceDidGeneratedIdentity {
                did: crate::ids::Did::parse("did:wba:awiki.test:alice:e1_new").unwrap(),
                unique_id: "e1_new".to_string(),
                did_document: serde_json::json!({
                    "id": "did:wba:awiki.test:alice:e1_new"
                }),
            },
            is_public: Some(false),
            is_agent: Some(true),
            role: Some(String::new()),
            endpoint_url: Some("https://example.test/agent".to_string()),
        }
    }

    fn identity_summary(did: &str) -> crate::identity::IdentitySummary {
        crate::identity::IdentitySummary {
            id: crate::ids::IdentityId::parse("alice-id").unwrap(),
            did: crate::ids::Did::parse(did).unwrap(),
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

    fn fixture_client() -> crate::core::ImClient {
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
                    identity_root_dir: std::path::PathBuf::from("/tmp/im-core-identities"),
                    registry_path: std::path::PathBuf::from(
                        "/tmp/im-core-identities/registry.json",
                    ),
                    default_identity_path: Some(std::path::PathBuf::from(
                        "/tmp/im-core-identities/default",
                    )),
                },
                local_state: crate::paths::LocalStatePaths {
                    sqlite_path: std::path::PathBuf::from("/tmp/im-core-local/im.sqlite"),
                },
                runtime: crate::paths::RuntimePaths {
                    cache_dir: std::path::PathBuf::from("/tmp/im-core-cache"),
                    temp_dir: std::path::PathBuf::from("/tmp/im-core-tmp"),
                },
            },
        )
        .unwrap();
        core.client(crate::identity::IdentitySelector::Did(
            crate::ids::Did::parse("did:wba:awiki.test:alice:e1_old").unwrap(),
        ))
        .unwrap()
    }
}
