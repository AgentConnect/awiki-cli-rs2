#[cfg(feature = "group-e2ee")]
use crate::internal::auth::session::SessionProvider;
#[cfg(feature = "group-e2ee")]
use crate::internal::group_e2ee::provider::GroupMlsProvider;
#[cfg(feature = "group-e2ee")]
use crate::internal::transport::AuthenticatedRpcTransport;

pub struct GroupService<'a> {
    client: &'a crate::core::ImClient,
}

impl<'a> GroupService<'a> {
    pub(crate) fn new(client: &'a crate::core::ImClient) -> Self {
        Self { client }
    }

    pub fn publish_key_package(
        &self,
        request: super::GroupKeyPackagePublishRequest,
    ) -> crate::ImResult<super::GroupKeyPackagePublishResult> {
        #[cfg(feature = "group-e2ee")]
        {
            self.publish_key_package_with_group_e2ee(request)
        }
        #[cfg(not(feature = "group-e2ee"))]
        {
            let _ = request;
            Err(crate::ImError::unsupported("group-e2ee"))
        }
    }

    pub fn create(
        &self,
        request: super::GroupCreateRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let secure_required = group_create_uses_e2ee(&request);
        #[cfg(not(feature = "group-e2ee"))]
        if secure_required {
            return Err(crate::ImError::unsupported("group-e2ee"));
        }
        #[cfg(feature = "group-e2ee")]
        let secure_provider = if secure_required {
            ensure_group_e2ee_service_available(self.client, false)?;
            Some(crate::internal::group_e2ee::storage::native_provider_for_client(self.client)?)
        } else {
            None
        };
        let mut result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .create(request, None)?;
        crate::internal::group_runtime::projection::project_group_snapshot(self.client, &result);
        self.refresh_group_state(&mut result, true);
        #[cfg(feature = "group-e2ee")]
        if secure_required {
            let group =
                group_did(&result).ok_or_else(|| crate::ImError::LocalStateUnavailable {
                    detail: "group E2EE create requires created group DID".to_owned(),
                })?;
            let secure = crate::internal::group_e2ee::lifecycle::GroupE2eeLifecycleRuntime::new(
                self.client,
                crate::internal::auth::session::FileSessionProvider::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
                secure_provider.expect("secure provider initialized when secure_required"),
            )
            .create_secure_group(
                crate::internal::group_e2ee::lifecycle::GroupE2eeCreateInput {
                    group: crate::ids::GroupRef::parse(&group)?,
                    credentials: None,
                    service_did: None,
                },
            )?;
            result.warnings.extend(secure.warnings);
        }
        Ok(result)
    }

    pub fn join(
        &self,
        request: super::GroupJoinRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let group = request.group.as_str().to_string();
        let mut result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .join(request, None)?;
        crate::internal::group_runtime::projection::project_group_snapshot(self.client, &result);
        self.refresh_group_state_for(&mut result, &group, true);
        Ok(result)
    }

    pub fn leave(
        &self,
        request: super::GroupLeaveRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let group = request.group.as_str().to_string();
        #[cfg(not(feature = "group-e2ee"))]
        if request.security.required() {
            return Err(crate::ImError::unsupported("group-e2ee"));
        }
        #[cfg(feature = "group-e2ee")]
        let secure_provider = if request.security.required() {
            ensure_group_e2ee_service_available(self.client, false)?;
            Some(crate::internal::group_e2ee::storage::native_provider_for_client(self.client)?)
        } else {
            None
        };
        if let Ok(Some(snapshot)) =
            crate::internal::group_runtime::cache::cached_group_snapshot(self.client, &group)
        {
            if crate::internal::group_runtime::cache::is_active_group_owner(&snapshot) {
                return Err(crate::ImError::invalid_input(
                    Some("group".to_string()),
                    "group owner cannot leave the group",
                ));
            }
            if crate::internal::group_runtime::cache::group_snapshot_uses_e2ee(&snapshot)
                && !request.security.required()
            {
                return Err(crate::ImError::unsupported("group-e2ee"));
            }
        }
        if request.security.required() {
            #[cfg(feature = "group-e2ee")]
            {
                let secure =
                    crate::internal::group_e2ee::lifecycle::GroupE2eeLifecycleRuntime::new(
                        self.client,
                        crate::internal::auth::session::FileSessionProvider::new(self.client),
                        crate::internal::transport::CoreHttpTransport::new(self.client),
                        secure_provider.expect("secure provider initialized when secure_required"),
                    )
                    .leave_secure_group(
                        crate::internal::group_e2ee::lifecycle::GroupE2eeLeaveInput {
                            group: request.group,
                            reason_text: request.reason_text,
                            owner_leave_commit: false,
                            credentials: None,
                        },
                    )?;
                return Ok(super::GroupReadResult::from_raw_response(
                    secure.delivery,
                    secure.warnings,
                ));
            }
        }
        let result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .leave(request, None)?;
        crate::internal::group_runtime::projection::project_group_left(self.client, &group);
        Ok(result)
    }

    pub fn add_member(
        &self,
        request: super::GroupMemberMutationRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let group = request.group.as_str().to_string();
        let resolved_member = resolve_group_member(self.client, &request.member)?;
        #[cfg(feature = "group-e2ee")]
        let member = resolved_member.did.as_str().to_string();
        #[cfg(feature = "group-e2ee")]
        let reason_text = request.reason_text.clone();
        #[cfg(not(feature = "group-e2ee"))]
        if request.security.required() {
            return Err(crate::ImError::unsupported("group-e2ee"));
        }
        #[cfg(feature = "group-e2ee")]
        let secure_provider = if request.security.required() {
            ensure_group_e2ee_service_available(self.client, true)?;
            Some(crate::internal::group_e2ee::storage::native_provider_for_client(self.client)?)
        } else {
            None
        };
        if let Ok(Some(snapshot)) = crate::internal::group_runtime::cache::cached_group_snapshot(
            self.client,
            request.group.as_str(),
        ) {
            if crate::internal::group_runtime::cache::group_snapshot_uses_e2ee(&snapshot)
                && !request.security.required()
            {
                return Err(crate::ImError::unsupported("group-e2ee"));
            }
        }
        let mut result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .add_member(
            resolved_group_member_request(request, &resolved_member),
            None,
        )?;
        result.resolved_member = Some(resolved_member.clone());
        crate::internal::group_runtime::projection::project_group_snapshot(self.client, &result);
        self.refresh_group_state_for(&mut result, &group, true);
        #[cfg(feature = "group-e2ee")]
        if let Some(secure_provider) = secure_provider {
            let secure = crate::internal::group_e2ee::lifecycle::GroupE2eeLifecycleRuntime::new(
                self.client,
                crate::internal::auth::session::FileSessionProvider::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
                secure_provider,
            )
            .add_secure_member(
                crate::internal::group_e2ee::lifecycle::GroupE2eeMemberMutationInput {
                    group: crate::ids::GroupRef::parse(&group)?,
                    member: crate::ids::Did::parse(&member)?,
                    reason_text,
                    leave_request_id: None,
                    credentials: None,
                    service_did: None,
                },
            )?;
            result.warnings.extend(secure.warnings);
        }
        Ok(result)
    }

    pub fn remove_member(
        &self,
        request: super::GroupMemberMutationRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let group = request.group.as_str().to_string();
        let resolved_member = resolve_group_member(self.client, &request.member)?;
        #[cfg(not(feature = "group-e2ee"))]
        if request.security.required() {
            return Err(crate::ImError::unsupported("group-e2ee"));
        }
        #[cfg(feature = "group-e2ee")]
        let secure_provider = if request.security.required() {
            ensure_group_e2ee_service_available(self.client, false)?;
            Some(crate::internal::group_e2ee::storage::native_provider_for_client(self.client)?)
        } else {
            None
        };
        if let Ok(Some(snapshot)) = crate::internal::group_runtime::cache::cached_group_snapshot(
            self.client,
            request.group.as_str(),
        ) {
            if crate::internal::group_runtime::cache::group_snapshot_uses_e2ee(&snapshot)
                && !request.security.required()
            {
                return Err(crate::ImError::unsupported("group-e2ee"));
            }
        }
        if request.security.required() {
            #[cfg(feature = "group-e2ee")]
            {
                let secure =
                    crate::internal::group_e2ee::lifecycle::GroupE2eeLifecycleRuntime::new(
                        self.client,
                        crate::internal::auth::session::FileSessionProvider::new(self.client),
                        crate::internal::transport::CoreHttpTransport::new(self.client),
                        secure_provider.expect("secure provider initialized when secure_required"),
                    )
                    .remove_secure_member(
                        crate::internal::group_e2ee::lifecycle::GroupE2eeMemberMutationInput {
                            group: request.group,
                            member: resolved_member.did.clone(),
                            reason_text: request.reason_text,
                            leave_request_id: None,
                            credentials: None,
                            service_did: None,
                        },
                    )?;
                let mut result =
                    super::GroupReadResult::from_raw_response(secure.delivery, secure.warnings);
                result.resolved_member = Some(resolved_member);
                self.refresh_group_state_for(&mut result, &group, true);
                return Ok(result);
            }
        }
        let mut result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .remove_member(
            resolved_group_member_request(request, &resolved_member),
            None,
        )?;
        result.resolved_member = Some(resolved_member);
        crate::internal::group_runtime::projection::project_group_snapshot(self.client, &result);
        self.refresh_group_state_for(&mut result, &group, true);
        Ok(result)
    }

    pub fn update_profile(
        &self,
        request: super::GroupUpdateProfileRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let group = request.group.as_str().to_string();
        if let Ok(Some(snapshot)) = crate::internal::group_runtime::cache::cached_group_snapshot(
            self.client,
            request.group.as_str(),
        ) {
            if crate::internal::group_runtime::cache::group_snapshot_uses_e2ee(&snapshot) {
                return Err(crate::ImError::unsupported("group-e2ee"));
            }
        }
        let mut result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .update_profile(request, None)?;
        crate::internal::group_runtime::projection::project_group_snapshot(self.client, &result);
        self.refresh_group_state_for(&mut result, &group, false);
        Ok(result)
    }

    pub fn update_policy(
        &self,
        request: super::GroupUpdatePolicyRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let group = request.group.as_str().to_string();
        if let Ok(Some(snapshot)) = crate::internal::group_runtime::cache::cached_group_snapshot(
            self.client,
            request.group.as_str(),
        ) {
            if crate::internal::group_runtime::cache::group_snapshot_uses_e2ee(&snapshot) {
                return Err(crate::ImError::unsupported("group-e2ee"));
            }
        }
        let mut result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .update_policy(request, None)?;
        crate::internal::group_runtime::projection::project_group_snapshot(self.client, &result);
        self.refresh_group_state_for(&mut result, &group, false);
        Ok(result)
    }

    pub fn update(
        &self,
        request: super::GroupUpdateRequest,
    ) -> crate::ImResult<super::GroupUpdateResult> {
        if request.profile_patch == super::GroupProfilePatch::default()
            && request.policy_patch == super::GroupPolicyPatch::default()
        {
            return Err(crate::ImError::invalid_input(
                Some("group_update".to_string()),
                "group update requires at least one mutable field",
            ));
        }
        if let Ok(Some(snapshot)) = crate::internal::group_runtime::cache::cached_group_snapshot(
            self.client,
            request.group.as_str(),
        ) {
            if crate::internal::group_runtime::cache::group_snapshot_uses_e2ee(&snapshot) {
                return Err(crate::ImError::unsupported("group-e2ee"));
            }
        }
        let group = request.group;
        let mut deliveries = Vec::new();
        let mut warnings = Vec::new();
        if request.profile_patch != super::GroupProfilePatch::default() {
            let result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
                self.client,
                crate::internal::auth::session::FileSessionProvider::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
            )
            .update_profile(
                super::GroupUpdateProfileRequest {
                    group: group.clone(),
                    patch: request.profile_patch,
                },
                None,
            )?;
            crate::internal::group_runtime::projection::project_group_snapshot(
                self.client,
                &result,
            );
            warnings.extend(result.warnings.iter().cloned());
            deliveries.push(result);
        }
        if request.policy_patch != super::GroupPolicyPatch::default() {
            let result = crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
                self.client,
                crate::internal::auth::session::FileSessionProvider::new(self.client),
                crate::internal::transport::CoreHttpTransport::new(self.client),
            )
            .update_policy(
                super::GroupUpdatePolicyRequest {
                    group: group.clone(),
                    patch: request.policy_patch,
                },
                None,
            )?;
            crate::internal::group_runtime::projection::project_group_snapshot(
                self.client,
                &result,
            );
            warnings.extend(result.warnings.iter().cloned());
            deliveries.push(result);
        }
        let mut refreshed = match self.get(group.clone()) {
            Ok(result) => Some(result),
            Err(err) => {
                warnings.push(format!("Failed to refresh group snapshot: {err}"));
                None
            }
        };
        if let Some(ref mut refreshed) = refreshed {
            warnings.extend(refreshed.warnings.iter().cloned());
        }
        Ok(super::GroupUpdateResult {
            deliveries,
            refreshed,
            warnings,
        })
    }

    pub fn get(&self, group: crate::ids::GroupRef) -> crate::ImResult<super::GroupReadResult> {
        let result = crate::internal::group_runtime::read::GroupReadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .get(group)?;
        crate::internal::group_runtime::projection::project_group_snapshot(self.client, &result);
        Ok(result)
    }

    pub fn list(
        &self,
        request: super::GroupListRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let result = crate::internal::group_runtime::read::GroupReadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .list(request)?;
        crate::internal::group_runtime::projection::project_group_summaries(self.client, &result);
        Ok(result)
    }

    pub fn members(
        &self,
        request: super::GroupMembersRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let group = request.group.as_str().to_string();
        let result = crate::internal::group_runtime::read::GroupReadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .members(request)?;
        crate::internal::group_runtime::projection::project_group_members(
            self.client,
            &group,
            &result,
        );
        Ok(result)
    }

    pub fn messages(
        &self,
        request: super::GroupMessagesRequest,
    ) -> crate::ImResult<super::GroupReadResult> {
        let group = request.group.as_str().to_string();
        let result = crate::internal::group_runtime::read::GroupReadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .messages(request)?;
        crate::internal::group_runtime::projection::project_group_messages(
            self.client,
            &group,
            &result,
        );
        Ok(result)
    }

    fn refresh_group_state(&self, result: &mut super::GroupReadResult, include_members: bool) {
        let Some(group) = group_did(result) else {
            return;
        };
        self.refresh_group_state_for(result, &group, include_members);
    }

    fn refresh_group_state_for(
        &self,
        result: &mut super::GroupReadResult,
        group: &str,
        include_members: bool,
    ) {
        let group_ref = match crate::ids::GroupRef::parse(group) {
            Ok(group_ref) => group_ref,
            Err(err) => {
                result.push_warning(format!("Failed to refresh group snapshot: {err}"));
                return;
            }
        };
        match self.get(group_ref.clone()) {
            Ok(snapshot) => result.merge_group_snapshot_from(&snapshot),
            Err(err) => {
                result.push_warning(format!("Failed to refresh group snapshot: {err}"));
                return;
            }
        }
        if !include_members {
            return;
        }
        match self.members(super::GroupMembersRequest {
            group: group_ref,
            limit: crate::ids::PageLimit(100),
        }) {
            Ok(members) => result.merge_group_members_from(&members),
            Err(err) => result.push_warning(format!("Failed to refresh group members: {err}")),
        }
    }

    #[cfg(feature = "group-e2ee")]
    fn publish_key_package_with_group_e2ee(
        &self,
        request: super::GroupKeyPackagePublishRequest,
    ) -> crate::ImResult<super::GroupKeyPackagePublishResult> {
        let session_provider =
            crate::internal::auth::session::FileSessionProvider::new(self.client);
        session_provider.ensure_session(crate::auth::AuthScope::GroupMessaging)?;
        let credentials = crate::internal::message_runtime::group::load_credentials(self.client)?;
        let service_did = group_e2ee_service_did(self.client)?;
        let operation_id = format!(
            "op-{}",
            crate::internal::wire::common::generate_operation_id()
        );
        let provider =
            crate::internal::group_e2ee::storage::native_provider_for_client(self.client)?;
        let device_id = request
            .device_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(crate::internal::group_e2ee::DEFAULT_GROUP_MLS_DEVICE_ID)
            .to_owned();
        let mut group_key_package = provider
            .generate_key_package(anp::group_e2ee::operations::GenerateKeyPackageInput {
                owner_did: self.client.did().as_str().to_owned(),
                device_id: device_id.clone(),
                operation_id: operation_id.clone(),
                request_id: format!("group-e2ee-key-package-{operation_id}"),
                key_package_id: request
                    .key_package_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned),
                purpose: Some(request.purpose.as_str().to_owned()),
                group_did: request
                    .group
                    .as_ref()
                    .map(|group| group.as_str().to_owned()),
            })?
            .group_key_package;
        group_key_package.did_wba_binding = sign_group_key_package_binding(
            &credentials,
            self.client.did().as_str(),
            group_key_package.did_wba_binding,
        )?;
        let params =
            crate::internal::group_e2ee::wire::build_group_e2ee_publish_key_package_rpc_params(
                &credentials,
                self.client.did().as_str(),
                service_did.as_str(),
                &group_key_package,
                &operation_id,
            )?;
        let mut transport = crate::internal::transport::CoreHttpTransport::new(self.client);
        let raw_response = transport.authenticated_rpc(
            crate::internal::message_runtime::group::MESSAGE_RPC_ENDPOINT,
            "group.e2ee.publish_key_package",
            params,
        )?;
        Ok(super::GroupKeyPackagePublishResult {
            owner_did: self.client.did().clone(),
            device_id,
            key_package_id: group_key_package.key_package_id,
            purpose: request.purpose,
            group: request.group,
            raw_response,
            warnings: Vec::new(),
        })
    }
}

fn group_did(result: &super::GroupReadResult) -> Option<String> {
    result
        .group
        .as_ref()
        .map(|group| group.did.as_str().to_string())
        .or_else(|| {
            result
                .response_json()
                .and_then(|raw| raw.get("group_did").or_else(|| raw.get("did")))
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
}

fn resolve_group_member(
    client: &crate::core::ImClient,
    member: &super::GroupMemberRef,
) -> crate::ImResult<super::GroupMemberResolution> {
    if member.is_did() {
        return Ok(super::GroupMemberResolution {
            did: member.as_did()?,
            handle: None,
        });
    }
    let handle = crate::ids::Handle::parse(member.as_str(), "")?;
    let lookup = client.directory().lookup_handle(handle)?;
    Ok(super::GroupMemberResolution {
        did: lookup.did,
        handle: Some(lookup.handle),
    })
}

fn resolved_group_member_request(
    request: super::GroupMemberMutationRequest,
    member: &super::GroupMemberResolution,
) -> super::GroupMemberMutationRequest {
    super::GroupMemberMutationRequest {
        group: request.group,
        member: super::GroupMemberRef::from(member.did.clone()),
        role: request.role,
        reason_text: request.reason_text,
        security: request.security,
    }
}

fn group_create_uses_e2ee(request: &super::GroupCreateRequest) -> bool {
    request.security.required()
        || request.e2ee
        || matches!(
            request.message_security_profile,
            Some(super::GroupMessageSecurityProfile::GroupE2ee)
        )
        || matches!(
            request.message_security_profile.as_ref(),
            Some(super::GroupMessageSecurityProfile::Custom(value))
                if value.trim() == "group-e2ee"
        )
}

#[cfg(feature = "group-e2ee")]
fn sign_group_key_package_binding(
    credentials: &crate::internal::message_runtime::group::GroupTextCredentials,
    owner_did: &str,
    binding: serde_json::Value,
) -> crate::ImResult<serde_json::Value> {
    let leaf_signature_key_b64u = binding
        .get("leaf_signature_key_b64u")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| crate::ImError::Serialization {
            detail: "group KeyPackage binding is missing leaf_signature_key_b64u".to_owned(),
        })?;
    let issued_at = binding
        .get("issued_at")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| crate::ImError::Serialization {
            detail: "group KeyPackage binding is missing issued_at".to_owned(),
        })?;
    let expires_at = binding
        .get("expires_at")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| crate::ImError::Serialization {
            detail: "group KeyPackage binding is missing expires_at".to_owned(),
        })?;
    let did_document =
        credentials
            .did_document
            .as_ref()
            .ok_or_else(|| crate::ImError::Serialization {
                detail: "identity is missing a DID document for group KeyPackage binding"
                    .to_owned(),
            })?;
    let verification_method = assertion_verification_method_id_from_document(did_document)
        .ok_or_else(|| crate::ImError::Serialization {
            detail: "identity is missing an assertion verification method".to_owned(),
        })?;
    let private_key =
        crate::internal::proof::origin::load_private_key_material(&credentials.key1_private_pem)?;
    anp::proof::generate_did_wba_binding(
        owner_did,
        &verification_method,
        leaf_signature_key_b64u,
        &private_key,
        issued_at,
        expires_at,
        Some(issued_at.to_owned()),
    )
    .map_err(|err| crate::ImError::Serialization {
        detail: format!("generate group KeyPackage DID WBA binding proof: {err}"),
    })
}

#[cfg(feature = "group-e2ee")]
fn assertion_verification_method_id_from_document(
    did_document: &serde_json::Value,
) -> Option<String> {
    did_document
        .get("assertionMethod")
        .and_then(serde_json::Value::as_array)
        .and_then(|methods| methods.first())
        .and_then(|method| {
            method
                .as_str()
                .or_else(|| method.get("id").and_then(serde_json::Value::as_str))
        })
        .map(str::to_owned)
        .or_else(|| {
            crate::internal::proof::origin::verification_method_id_from_document(did_document)
        })
}

#[cfg(feature = "group-e2ee")]
fn ensure_group_e2ee_service_available(
    client: &crate::core::ImClient,
    check_key_package: bool,
) -> crate::ImResult<()> {
    let session_provider = crate::internal::auth::session::FileSessionProvider::new(client);
    let mut transport = crate::internal::transport::CoreHttpTransport::new(client);
    crate::internal::group_e2ee::lifecycle::ensure_group_e2ee_service_available(
        client,
        &session_provider,
        &mut transport,
        crate::internal::group_e2ee::lifecycle::GroupE2eeServiceAvailabilityInput {
            credentials: None,
            service_did: None,
            check_key_package,
        },
    )
}

#[cfg(feature = "group-e2ee")]
fn group_e2ee_service_did(client: &crate::core::ImClient) -> crate::ImResult<crate::ids::Did> {
    client
        .core_inner()
        .sdk_config()
        .anp_service_did
        .clone()
        .ok_or_else(|| {
            crate::ImError::invalid_input(
                Some("anp_service_did".to_owned()),
                "group E2EE key package publish requires ImCoreConfig.anp_service_did",
            )
        })
}
