use serde_json::Value;

use crate::internal::transport::RpcTransport;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DirectoryResolveResult {
    pub(crate) resolution: crate::directory::DirectoryResolution,
    pub(crate) resolve: Option<Value>,
    pub(crate) lookup: Option<Value>,
    pub(crate) public_profile: Option<Value>,
}

pub(crate) struct DirectoryRuntime<'a, T> {
    client: &'a crate::core::ImClient,
    transport: T,
}

impl<'a, T> DirectoryRuntime<'a, T>
where
    T: RpcTransport,
{
    pub(crate) fn new(client: &'a crate::core::ImClient, transport: T) -> Self {
        Self { client, transport }
    }

    pub(crate) fn lookup_handle(
        mut self,
        handle: crate::ids::Handle,
    ) -> crate::ImResult<crate::directory::HandleLookupResult> {
        let raw = lookup_by_handle(&mut self.transport, handle.as_str())?;
        handle_lookup_from_value(&raw)
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
        let lookup_raw = lookup_by_handle(&mut self.transport, &handle)?;
        let lookup = handle_lookup_from_value(&lookup_raw)?;
        let profile = match public_profile_by_did(&mut self.transport, lookup.did.as_str()) {
            Ok(profile) => Some(profile),
            Err(_) => None,
        };
        let resolve_raw = resolve_profile_by_did(&mut self.transport, lookup.did.as_str())?;
        let profile_dto = profile
            .as_ref()
            .map(|raw| crate::internal::profile_runtime::profile_from_value(self.client, raw))
            .transpose()?;
        Ok(DirectoryResolveResult {
            resolution: crate::directory::DirectoryResolution {
                input: handle,
                did: lookup.did.clone(),
                handle: Some(lookup.handle),
                profile: profile_dto,
                warnings: Vec::new(),
            },
            resolve: Some(resolve_raw),
            lookup: Some(lookup_raw),
            public_profile: profile,
        })
    }

    fn resolve_did(&mut self, did: String) -> crate::ImResult<DirectoryResolveResult> {
        let resolve_raw = resolve_profile_by_did(&mut self.transport, &did)?;
        let did = crate::ids::Did::parse(did)?;
        let mut warnings = Vec::new();
        let mut lookup_raw = None;
        let mut handle = None;
        match lookup_by_did(&mut self.transport, did.as_str()) {
            Ok(raw) => {
                let lookup = handle_lookup_from_value(&raw)?;
                handle = Some(lookup.handle);
                lookup_raw = Some(raw);
            }
            Err(err) => warnings.push(format!("Handle lookup failed: {err}")),
        }
        let mut profile_raw = None;
        let mut profile = None;
        match public_profile_by_did(&mut self.transport, did.as_str()) {
            Ok(raw) => {
                profile = Some(crate::internal::profile_runtime::profile_from_value(
                    self.client,
                    &raw,
                )?);
                profile_raw = Some(raw);
            }
            Err(err) => warnings.push(format!("Public profile lookup failed: {err}")),
        }
        Ok(DirectoryResolveResult {
            resolution: crate::directory::DirectoryResolution {
                input: did.as_str().to_string(),
                did,
                handle,
                profile,
                warnings,
            },
            resolve: Some(resolve_raw),
            lookup: lookup_raw,
            public_profile: profile_raw,
        })
    }
}

fn lookup_by_handle<T>(transport: &mut T, handle: &str) -> crate::ImResult<Value>
where
    T: RpcTransport,
{
    let call =
        crate::internal::identity_wire::directory::build_handle_lookup_by_handle_rpc_call(handle)?;
    transport.rpc(call.endpoint, call.method, call.params)
}

fn lookup_by_did<T>(transport: &mut T, did: &str) -> crate::ImResult<Value>
where
    T: RpcTransport,
{
    let call = crate::internal::identity_wire::directory::build_handle_lookup_by_did_rpc_call(did)?;
    transport.rpc(call.endpoint, call.method, call.params)
}

fn resolve_profile_by_did<T>(transport: &mut T, did: &str) -> crate::ImResult<Value>
where
    T: RpcTransport,
{
    let call = crate::internal::identity_wire::profile::build_profile_resolve_rpc_call(did)?;
    transport.rpc(call.endpoint, call.method, call.params)
}

fn public_profile_by_did<T>(transport: &mut T, did: &str) -> crate::ImResult<Value>
where
    T: RpcTransport,
{
    let call = crate::internal::identity_wire::profile::build_public_profile_rpc_call(did)?;
    transport.rpc(call.endpoint, call.method, call.params)
}

fn handle_lookup_from_value(
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
    Ok(crate::directory::HandleLookupResult {
        handle: crate::ids::Handle::parse(handle, "")?,
        did: crate::ids::Did::parse(did)?,
        domain: string_option(value, "domain"),
        status: string_option(value, "status"),
    })
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
