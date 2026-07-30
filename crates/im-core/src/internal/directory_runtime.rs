use serde_json::Value;

use crate::internal::transport::{AsyncRpcTransport, RpcTransport};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DirectoryResolveResult {
    pub(crate) resolution: crate::directory::DirectoryResolution,
    pub(crate) handle_lookup: Option<crate::directory::HandleLookupResult>,
    pub(crate) resolve: Option<Value>,
    pub(crate) lookup: Option<Value>,
    pub(crate) public_profile: Option<Value>,
}

pub(crate) struct DirectoryRuntime<'a, T> {
    client: &'a crate::core::ImClient,
    transport: T,
}

impl<'a, T> DirectoryRuntime<'a, T> {
    pub(crate) fn new(client: &'a crate::core::ImClient, transport: T) -> Self {
        Self { client, transport }
    }
}

impl<'a, T> DirectoryRuntime<'a, T>
where
    T: RpcTransport,
{
    pub(crate) fn lookup_handle(
        mut self,
        handle: crate::ids::Handle,
    ) -> crate::ImResult<crate::directory::HandleLookupResult> {
        let raw = lookup_by_handle(&mut self.transport, handle.as_str())?;
        handle_lookup_from_value_with_client(self.client, &raw)
    }

    pub(crate) fn public_profile(
        mut self,
        subject: crate::directory::IdentitySubject,
    ) -> crate::ImResult<crate::directory::PublicProfile> {
        match subject {
            crate::directory::IdentitySubject::Did(did) => self.public_profile_by_did(
                crate::directory::IdentitySubject::Did(did.clone()),
                did,
                None,
            ),
            crate::directory::IdentitySubject::Handle(handle) => {
                self.public_profile_by_handle(handle)
            }
            crate::directory::IdentitySubject::Any(input) => {
                if input.trim().starts_with("did:") {
                    let did = crate::ids::Did::parse(input.trim())?;
                    self.public_profile_by_did(
                        crate::directory::IdentitySubject::Did(did.clone()),
                        did,
                        None,
                    )
                } else {
                    let handle = crate::ids::Handle::parse(input.trim(), "")?;
                    self.public_profile_by_handle(handle)
                }
            }
        }
    }

    pub(crate) fn resolve_peer(
        mut self,
        peer: crate::ids::PeerRef,
    ) -> crate::ImResult<DirectoryResolveResult> {
        let input = peer.as_str().trim().to_string();
        if input.is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("peer".to_string()),
                "peer must not be empty",
            ));
        }

        if input.starts_with("did:") {
            self.resolve_did(input)
        } else {
            self.resolve_handle(input)
        }
    }

    fn resolve_handle(&mut self, handle: String) -> crate::ImResult<DirectoryResolveResult> {
        let lookup_raw = lookup_by_handle(&mut self.transport, &handle)
            .map_err(|err| map_directory_not_found(err, &handle))?;
        let lookup = handle_lookup_from_value_with_client(self.client, &lookup_raw)?;
        let conversation_id = lookup.direct_conversation_id();
        let warnings = lookup.warnings.clone();
        let fallback_profile = if lookup.profile.is_none() {
            public_profile_by_did(&mut self.transport, lookup.did.as_str()).ok()
        } else {
            None
        };
        let resolve_raw = resolve_profile_by_did(&mut self.transport, lookup.did.as_str())?;
        let profile_dto = lookup
            .profile
            .clone()
            .map(Ok)
            .or_else(|| {
                fallback_profile.as_ref().map(|raw| {
                    let mut profile =
                        crate::internal::profile_runtime::profile_from_value(self.client, raw)?;
                    profile.subject = lookup.did.clone();
                    Ok::<_, crate::ImError>(profile)
                })
            })
            .transpose()?;
        Ok(DirectoryResolveResult {
            resolution: crate::directory::DirectoryResolution {
                input: handle,
                did: lookup.did.clone(),
                handle: Some(lookup.handle.clone()),
                conversation_id,
                profile: profile_dto,
                warnings,
            },
            handle_lookup: Some(lookup.clone()),
            resolve: Some(resolve_raw),
            lookup: Some(lookup_raw),
            public_profile: fallback_profile,
        })
    }

    fn resolve_did(&mut self, did: String) -> crate::ImResult<DirectoryResolveResult> {
        let resolve_raw = resolve_profile_by_did(&mut self.transport, &did)?;
        let did = crate::ids::Did::parse(did)?;
        let mut warnings = Vec::new();
        let mut lookup_raw = None;
        let mut handle = None;
        let mut conversation_id =
            crate::internal::local_state::owner_scope::direct_conversation_id(did.as_str());
        match lookup_by_did(&mut self.transport, did.as_str()) {
            Ok(raw) => {
                let lookup = handle_lookup_from_value_with_client(self.client, &raw)?;
                conversation_id = lookup.direct_conversation_id();
                warnings.extend(lookup.warnings);
                handle = Some(lookup.handle);
                lookup_raw = Some(raw);
            }
            Err(err) => warnings.push(format!("Handle lookup failed: {err}")),
        }
        let mut profile_raw = None;
        let mut profile = None;
        match public_profile_by_did(&mut self.transport, did.as_str()) {
            Ok(raw) => {
                let mut value =
                    crate::internal::profile_runtime::profile_from_value(self.client, &raw)?;
                value.subject = did.clone();
                profile = Some(value);
                profile_raw = Some(raw);
            }
            Err(err) => warnings.push(format!("Public profile lookup failed: {err}")),
        }
        Ok(DirectoryResolveResult {
            resolution: crate::directory::DirectoryResolution {
                input: did.as_str().to_string(),
                did,
                handle,
                conversation_id,
                profile,
                warnings,
            },
            handle_lookup: lookup_raw
                .as_ref()
                .map(|raw| handle_lookup_from_value_with_client(self.client, raw))
                .transpose()?,
            resolve: Some(resolve_raw),
            lookup: lookup_raw,
            public_profile: profile_raw,
        })
    }

    fn public_profile_by_handle(
        &mut self,
        handle: crate::ids::Handle,
    ) -> crate::ImResult<crate::directory::PublicProfile> {
        let lookup_raw = lookup_by_handle(&mut self.transport, handle.as_str())
            .map_err(|err| map_directory_not_found(err, handle.as_str()))?;
        let lookup = handle_lookup_from_value_with_client(self.client, &lookup_raw)?;
        if let Some(profile) = lookup.profile {
            return Ok(crate::directory::PublicProfile {
                subject: crate::directory::IdentitySubject::Handle(handle),
                did: lookup.did,
                handle: Some(lookup.handle.clone()),
                profile,
                warnings: lookup.warnings,
            });
        }
        self.public_profile_by_did(
            crate::directory::IdentitySubject::Handle(handle),
            lookup.did,
            Some(lookup.handle),
        )
    }

    fn public_profile_by_did(
        &mut self,
        subject: crate::directory::IdentitySubject,
        did: crate::ids::Did,
        handle: Option<crate::ids::Handle>,
    ) -> crate::ImResult<crate::directory::PublicProfile> {
        let raw = public_profile_by_did(&mut self.transport, did.as_str())?;
        let mut profile = crate::internal::profile_runtime::profile_from_value(self.client, &raw)?;
        profile.subject = did.clone();
        Ok(crate::directory::PublicProfile {
            subject,
            did,
            handle: handle.or_else(|| profile.handle.clone()),
            profile,
            warnings: Vec::new(),
        })
    }
}

impl<'a, T> DirectoryRuntime<'a, T>
where
    T: AsyncRpcTransport,
{
    pub(crate) async fn lookup_handle_async(
        mut self,
        handle: crate::ids::Handle,
    ) -> crate::ImResult<crate::directory::HandleLookupResult> {
        let raw = lookup_by_handle_async(&mut self.transport, handle.as_str()).await?;
        handle_lookup_from_value_with_client(self.client, &raw)
    }

    pub(crate) async fn public_profile_async(
        mut self,
        subject: crate::directory::IdentitySubject,
    ) -> crate::ImResult<crate::directory::PublicProfile> {
        match subject {
            crate::directory::IdentitySubject::Did(did) => {
                self.public_profile_by_did_async(
                    crate::directory::IdentitySubject::Did(did.clone()),
                    did,
                    None,
                )
                .await
            }
            crate::directory::IdentitySubject::Handle(handle) => {
                self.public_profile_by_handle_async(handle).await
            }
            crate::directory::IdentitySubject::Any(input) => {
                if input.trim().starts_with("did:") {
                    let did = crate::ids::Did::parse(input.trim())?;
                    self.public_profile_by_did_async(
                        crate::directory::IdentitySubject::Did(did.clone()),
                        did,
                        None,
                    )
                    .await
                } else {
                    let handle = crate::ids::Handle::parse(input.trim(), "")?;
                    self.public_profile_by_handle_async(handle).await
                }
            }
        }
    }

    pub(crate) async fn resolve_peer_async(
        mut self,
        peer: crate::ids::PeerRef,
    ) -> crate::ImResult<DirectoryResolveResult> {
        let input = peer.as_str().trim().to_string();
        if input.is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("peer".to_string()),
                "peer must not be empty",
            ));
        }

        if input.starts_with("did:") {
            self.resolve_did_async(input).await
        } else {
            self.resolve_handle_async(input).await
        }
    }

    async fn resolve_handle_async(
        &mut self,
        handle: String,
    ) -> crate::ImResult<DirectoryResolveResult> {
        let lookup_raw = lookup_by_handle_async(&mut self.transport, &handle)
            .await
            .map_err(|err| map_directory_not_found(err, &handle))?;
        let lookup = handle_lookup_from_value_with_client(self.client, &lookup_raw)?;
        let conversation_id = lookup.direct_conversation_id();
        let warnings = lookup.warnings.clone();
        let fallback_profile = if lookup.profile.is_none() {
            public_profile_by_did_async(&mut self.transport, lookup.did.as_str())
                .await
                .ok()
        } else {
            None
        };
        let resolve_raw =
            resolve_profile_by_did_async(&mut self.transport, lookup.did.as_str()).await?;
        let profile_dto = lookup
            .profile
            .clone()
            .map(Ok)
            .or_else(|| {
                fallback_profile.as_ref().map(|raw| {
                    let mut profile =
                        crate::internal::profile_runtime::profile_from_value(self.client, raw)?;
                    profile.subject = lookup.did.clone();
                    Ok::<_, crate::ImError>(profile)
                })
            })
            .transpose()?;
        Ok(DirectoryResolveResult {
            resolution: crate::directory::DirectoryResolution {
                input: handle,
                did: lookup.did.clone(),
                handle: Some(lookup.handle.clone()),
                conversation_id,
                profile: profile_dto,
                warnings,
            },
            handle_lookup: Some(lookup.clone()),
            resolve: Some(resolve_raw),
            lookup: Some(lookup_raw),
            public_profile: fallback_profile,
        })
    }

    async fn resolve_did_async(&mut self, did: String) -> crate::ImResult<DirectoryResolveResult> {
        let resolve_raw = resolve_profile_by_did_async(&mut self.transport, &did).await?;
        let did = crate::ids::Did::parse(did)?;
        let mut warnings = Vec::new();
        let mut lookup_raw = None;
        let mut handle = None;
        let mut conversation_id =
            crate::internal::local_state::owner_scope::direct_conversation_id(did.as_str());
        match lookup_by_did_async(&mut self.transport, did.as_str()).await {
            Ok(raw) => {
                let lookup = handle_lookup_from_value_with_client(self.client, &raw)?;
                conversation_id = lookup.direct_conversation_id();
                warnings.extend(lookup.warnings);
                handle = Some(lookup.handle);
                lookup_raw = Some(raw);
            }
            Err(err) => warnings.push(format!("Handle lookup failed: {err}")),
        }
        let mut profile_raw = None;
        let mut profile = None;
        match public_profile_by_did_async(&mut self.transport, did.as_str()).await {
            Ok(raw) => {
                let mut value =
                    crate::internal::profile_runtime::profile_from_value(self.client, &raw)?;
                value.subject = did.clone();
                profile = Some(value);
                profile_raw = Some(raw);
            }
            Err(err) => warnings.push(format!("Public profile lookup failed: {err}")),
        }
        Ok(DirectoryResolveResult {
            resolution: crate::directory::DirectoryResolution {
                input: did.as_str().to_string(),
                did,
                handle,
                conversation_id,
                profile,
                warnings,
            },
            handle_lookup: lookup_raw
                .as_ref()
                .map(|raw| handle_lookup_from_value_with_client(self.client, raw))
                .transpose()?,
            resolve: Some(resolve_raw),
            lookup: lookup_raw,
            public_profile: profile_raw,
        })
    }

    async fn public_profile_by_handle_async(
        &mut self,
        handle: crate::ids::Handle,
    ) -> crate::ImResult<crate::directory::PublicProfile> {
        let lookup_raw = lookup_by_handle_async(&mut self.transport, handle.as_str())
            .await
            .map_err(|err| map_directory_not_found(err, handle.as_str()))?;
        let lookup = handle_lookup_from_value_with_client(self.client, &lookup_raw)?;
        if let Some(profile) = lookup.profile {
            return Ok(crate::directory::PublicProfile {
                subject: crate::directory::IdentitySubject::Handle(handle),
                did: lookup.did,
                handle: Some(lookup.handle),
                profile,
                warnings: lookup.warnings,
            });
        }
        self.public_profile_by_did_async(
            crate::directory::IdentitySubject::Handle(handle),
            lookup.did,
            Some(lookup.handle),
        )
        .await
    }

    async fn public_profile_by_did_async(
        &mut self,
        subject: crate::directory::IdentitySubject,
        did: crate::ids::Did,
        handle: Option<crate::ids::Handle>,
    ) -> crate::ImResult<crate::directory::PublicProfile> {
        let raw = public_profile_by_did_async(&mut self.transport, did.as_str()).await?;
        let mut profile = crate::internal::profile_runtime::profile_from_value(self.client, &raw)?;
        profile.subject = did.clone();
        Ok(crate::directory::PublicProfile {
            subject,
            did,
            handle: handle.or_else(|| profile.handle.clone()),
            profile,
            warnings: Vec::new(),
        })
    }
}

fn lookup_by_handle<T>(transport: &mut T, handle: &str) -> crate::ImResult<Value>
where
    T: RpcTransport,
{
    let call =
        crate::internal::identity_wire::directory::build_handle_lookup_by_handle_rpc_call(handle)?;
    crate::internal::idempotent_submission::submit_read_rpc(
        transport,
        call.endpoint,
        call.method,
        call.params,
    )
}

async fn lookup_by_handle_async<T>(transport: &mut T, handle: &str) -> crate::ImResult<Value>
where
    T: AsyncRpcTransport,
{
    let call =
        crate::internal::identity_wire::directory::build_handle_lookup_by_handle_rpc_call(handle)?;
    crate::internal::idempotent_submission::submit_read_rpc_async(
        transport,
        call.endpoint,
        call.method,
        call.params,
    )
    .await
}

fn map_directory_not_found(err: crate::ImError, peer: &str) -> crate::ImError {
    if service_error_is_not_found(&err) {
        crate::ImError::PeerNotFound {
            peer: peer.to_string(),
        }
    } else {
        err
    }
}

fn service_error_is_not_found(err: &crate::ImError) -> bool {
    let crate::ImError::Service {
        status_code,
        code,
        message,
        ..
    } = err
    else {
        return false;
    };
    if matches!(status_code, Some(404)) {
        return true;
    }
    if code
        .as_deref()
        .is_some_and(|code| matches!(code, "404" | "-32004" | "not_found" | "handle_not_found"))
    {
        return true;
    }
    let normalized = message.trim().to_ascii_lowercase();
    normalized.contains("not found") || message.contains("不存在")
}

fn lookup_by_did<T>(transport: &mut T, did: &str) -> crate::ImResult<Value>
where
    T: RpcTransport,
{
    let call = crate::internal::identity_wire::directory::build_handle_lookup_by_did_rpc_call(did)?;
    crate::internal::idempotent_submission::submit_read_rpc(
        transport,
        call.endpoint,
        call.method,
        call.params,
    )
}

async fn lookup_by_did_async<T>(transport: &mut T, did: &str) -> crate::ImResult<Value>
where
    T: AsyncRpcTransport,
{
    let call = crate::internal::identity_wire::directory::build_handle_lookup_by_did_rpc_call(did)?;
    crate::internal::idempotent_submission::submit_read_rpc_async(
        transport,
        call.endpoint,
        call.method,
        call.params,
    )
    .await
}

pub(crate) async fn lookup_handle_by_did_for_projection_async<T>(
    client: &crate::core::ImClient,
    transport: &mut T,
    did: &crate::ids::Did,
) -> crate::ImResult<crate::directory::HandleLookupResult>
where
    T: AsyncRpcTransport,
{
    let raw = lookup_by_did_async(transport, did.as_str()).await?;
    let lookup = handle_lookup_from_value_with_client(client, &raw)?;
    if lookup.did != *did {
        return Err(crate::ImError::IdentityBindingConflict {
            detail: "DID lookup returned a different verified identity".to_owned(),
        });
    }
    Ok(lookup)
}

fn resolve_profile_by_did<T>(transport: &mut T, did: &str) -> crate::ImResult<Value>
where
    T: RpcTransport,
{
    let call = crate::internal::identity_wire::profile::build_profile_resolve_rpc_call(did)?;
    transport.rpc(call.endpoint, call.method, call.params)
}

async fn resolve_profile_by_did_async<T>(transport: &mut T, did: &str) -> crate::ImResult<Value>
where
    T: AsyncRpcTransport,
{
    let call = crate::internal::identity_wire::profile::build_profile_resolve_rpc_call(did)?;
    transport.rpc(call.endpoint, call.method, call.params).await
}

fn public_profile_by_did<T>(transport: &mut T, did: &str) -> crate::ImResult<Value>
where
    T: RpcTransport,
{
    let call = crate::internal::identity_wire::profile::build_public_profile_rpc_call(did)?;
    transport.rpc(call.endpoint, call.method, call.params)
}

async fn public_profile_by_did_async<T>(transport: &mut T, did: &str) -> crate::ImResult<Value>
where
    T: AsyncRpcTransport,
{
    let call = crate::internal::identity_wire::profile::build_public_profile_rpc_call(did)?;
    transport.rpc(call.endpoint, call.method, call.params).await
}

pub(crate) fn handle_lookup_from_value_with_client(
    client: &crate::core::ImClient,
    value: &Value,
) -> crate::ImResult<crate::directory::HandleLookupResult> {
    let did = string_value(value, "did");
    if did.trim().is_empty() {
        return Err(crate::ImError::PeerNotFound {
            peer: "handle lookup".to_string(),
        });
    }
    let handle = first_string_value(value, &["full_handle", "handle"]);
    if handle.trim().is_empty() {
        return Err(crate::ImError::PeerNotFound { peer: did.clone() });
    }
    let did = crate::ids::Did::parse(did)?;
    let handle = crate::ids::Handle::parse(handle, "")?;
    let user_id = stable_user_id_from_lookup(value)?;
    let domain = string_option(value, "domain");
    let status = string_option(value, "status");
    crate::internal::canonical_identity::PeerPersona::from_verified_handle(
        domain.as_deref().unwrap_or_default(),
        &user_id,
        handle.as_str(),
        status.as_deref(),
    )?;
    let (profile, warnings) = profile_from_lookup(client, value.get("profile"), &did, &handle)?;
    Ok(crate::directory::HandleLookupResult {
        handle,
        did,
        user_id,
        domain,
        status,
        binding_generation: string_option(value, "binding_generation"),
        profile,
        warnings,
    })
}

pub(crate) fn handle_lookup_from_value(
    value: &Value,
) -> crate::ImResult<crate::directory::HandleLookupResult> {
    let did = string_value(value, "did");
    if did.trim().is_empty() {
        return Err(crate::ImError::PeerNotFound {
            peer: "handle lookup".to_string(),
        });
    }
    let handle = first_string_value(value, &["full_handle", "handle"]);
    if handle.trim().is_empty() {
        return Err(crate::ImError::PeerNotFound { peer: did.clone() });
    }
    let user_id = stable_user_id_from_lookup(value)?;
    let domain = string_option(value, "domain");
    let status = string_option(value, "status");
    let handle = crate::ids::Handle::parse(handle, "")?;
    crate::internal::canonical_identity::PeerPersona::from_verified_handle(
        domain.as_deref().unwrap_or_default(),
        &user_id,
        handle.as_str(),
        status.as_deref(),
    )?;
    Ok(crate::directory::HandleLookupResult {
        handle,
        did: crate::ids::Did::parse(did)?,
        user_id,
        domain,
        status,
        binding_generation: string_option(value, "binding_generation"),
        profile: None,
        warnings: Vec::new(),
    })
}

fn stable_user_id_from_lookup(value: &Value) -> crate::ImResult<String> {
    let value = first_string_value(value, &["user_id", "userId", "subject_id", "subjectId"]);
    let value = value.trim();
    if value.is_empty() {
        Err(crate::ImError::IdentityUnresolved {
            detail: "Handle authority did not return a stable user_id/subject_id".to_owned(),
        })
    } else {
        Ok(value.to_owned())
    }
}

fn profile_from_lookup(
    client: &crate::core::ImClient,
    value: Option<&Value>,
    did: &crate::ids::Did,
    handle: &crate::ids::Handle,
) -> crate::ImResult<(Option<crate::identity::Profile>, Vec<String>)> {
    let Some(value) = value else {
        return Ok((None, Vec::new()));
    };
    if !value.is_object() {
        return Ok((
            None,
            vec!["Ignoring WNS profile because it is not a JSON object".to_string()],
        ));
    }
    let explicit_subject = first_string_value(value, &["subject_did", "did", "subject", "id"]);
    if explicit_subject.trim().is_empty() {
        return Ok((
            None,
            vec!["Ignoring WNS profile because profile.subject_did is missing".to_string()],
        ));
    }
    if explicit_subject != did.as_str() {
        return Ok((
            None,
            vec![
                "Ignoring WNS profile because profile.subject_did does not match resolved did"
                    .to_string(),
            ],
        ));
    }
    let mut profile = crate::internal::profile_runtime::profile_from_value(client, value)?;
    let mut warnings = Vec::new();
    if profile.subject != *did {
        warnings.push(
            "Ignoring WNS profile because profile.subject_did does not match resolved did"
                .to_string(),
        );
        return Ok((None, warnings));
    }
    if let Some(profile_handle) = profile.handle.as_ref() {
        if profile_handle != handle {
            warnings.push(
                "Ignoring WNS profile because profile.handle does not match resolved handle"
                    .to_string(),
            );
            return Ok((None, warnings));
        }
    } else {
        profile.handle = Some(handle.clone());
    }
    Ok((Some(profile), warnings))
}

fn first_string_value(value: &Value, keys: &[&str]) -> String {
    keys.iter()
        .map(|key| string_value(value, key))
        .find(|value| !value.trim().is_empty())
        .unwrap_or_default()
}

fn string_value(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn string_option(value: &Value, key: &str) -> Option<String> {
    let value = string_value(value, key);
    (!value.trim().is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct RecordingTransport {
        calls: Vec<(String, String, Value)>,
        results: Vec<crate::ImResult<Value>>,
    }

    impl RpcTransport for RecordingTransport {
        fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
            self.calls
                .push((endpoint.to_owned(), method.to_owned(), params));
            self.results.remove(0)
        }
    }

    impl AsyncRpcTransport for RecordingTransport {
        async fn rpc(
            &mut self,
            endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            RpcTransport::rpc(self, endpoint, method, params)
        }
    }

    #[tokio::test]
    async fn handle_lookup_async_replays_a_transport_failure_once() {
        let mut transport = RecordingTransport {
            calls: Vec::new(),
            results: vec![transport_unavailable(), Ok(handle_lookup_value())],
        };

        let result = lookup_by_handle_async(&mut transport, "alice.awiki.info")
            .await
            .unwrap();

        assert_eq!(result, handle_lookup_value());
        assert_eq!(transport.calls.len(), 2);
        assert_eq!(transport.calls[0], transport.calls[1]);
        assert_eq!(
            transport.calls[0],
            (
                "/user-service/handle/rpc".to_owned(),
                "lookup".to_owned(),
                json!({"handle": "alice.awiki.info"}),
            )
        );
    }

    #[test]
    fn handle_lookup_preserves_authoritative_binding_generation() {
        let lookup = handle_lookup_from_value(&json!({
            "did": "did:wba:awiki.info:alice:e1_current",
            "full_handle": "alice.awiki.info",
            "user_id": "user-alice",
            "domain": "awiki.info",
            "status": "active",
            "binding_generation": "3"
        }))
        .unwrap();

        assert_eq!(lookup.binding_generation.as_deref(), Some("3"));
    }

    #[test]
    fn handle_lookup_does_not_coerce_numeric_binding_generation() {
        let lookup = handle_lookup_from_value(&json!({
            "did": "did:wba:awiki.info:alice:e1_current",
            "full_handle": "alice.awiki.info",
            "user_id": "user-alice",
            "domain": "awiki.info",
            "status": "active",
            "binding_generation": 3
        }))
        .unwrap();

        assert_eq!(lookup.binding_generation, None);
    }

    fn handle_lookup_value() -> Value {
        json!({
            "did": "did:wba:awiki.info:alice:e1_current",
            "full_handle": "alice.awiki.info",
            "user_id": "user-alice",
            "domain": "awiki.info",
            "status": "active",
            "binding_generation": "3"
        })
    }

    fn transport_unavailable() -> crate::ImResult<Value> {
        Err(crate::ImError::TransportUnavailable {
            detail: "connection reset".to_owned(),
        })
    }
}
