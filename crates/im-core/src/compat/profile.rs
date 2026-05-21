//! Migration-only profile runtime bridge for `awiki-cli`.

use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct ProfileReadBridgeResult {
    pub profile: crate::identity::Profile,
    pub raw: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProfileUpdateBridgeResult {
    pub profile: crate::identity::Profile,
    pub raw: Value,
    pub changed_fields: Vec<String>,
}

pub trait BridgeProfileSessionProvider {
    fn ensure_profile_session(&self) -> crate::ImResult<crate::auth::SessionBundle>;
}

pub trait BridgeProfileAuthenticatedRpcTransport {
    fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value>;
}

#[doc(hidden)]
pub fn read_self_profile_with_bridge<P, T>(
    client: &crate::core::ImClient,
    session_provider: P,
    transport: T,
) -> crate::ImResult<ProfileReadBridgeResult>
where
    P: BridgeProfileSessionProvider,
    T: BridgeProfileAuthenticatedRpcTransport,
{
    let result = client.identity().profile_with_runtime(
        CompatProfileSessionProvider(session_provider),
        CompatProfileTransport(transport),
    )?;
    Ok(ProfileReadBridgeResult {
        profile: result.profile,
        raw: result.raw,
    })
}

#[doc(hidden)]
pub fn update_profile_with_bridge<P, T>(
    client: &crate::core::ImClient,
    patch: crate::identity::ProfilePatch,
    session_provider: P,
    transport: T,
) -> crate::ImResult<ProfileUpdateBridgeResult>
where
    P: BridgeProfileSessionProvider,
    T: BridgeProfileAuthenticatedRpcTransport,
{
    let result = client.identity().update_profile_with_runtime(
        patch,
        CompatProfileSessionProvider(session_provider),
        CompatProfileTransport(transport),
    )?;
    Ok(ProfileUpdateBridgeResult {
        profile: result.profile,
        raw: result.raw,
        changed_fields: result.changed_fields,
    })
}

struct CompatProfileSessionProvider<P>(P);

impl<P> crate::internal::auth::session::SessionProvider for CompatProfileSessionProvider<P>
where
    P: BridgeProfileSessionProvider,
{
    fn ensure_session(
        &self,
        scope: crate::auth::AuthScope,
    ) -> crate::ImResult<crate::auth::SessionBundle> {
        if scope != crate::auth::AuthScope::UserProfile {
            return Err(crate::ImError::unsupported("auth-scope"));
        }
        self.0.ensure_profile_session()
    }

    fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
        Err(crate::ImError::TransportUnavailable {
            detail: "refresh is owned by the bridge transport in Phase 2".to_string(),
        })
    }

    fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
        Err(crate::ImError::TransportUnavailable {
            detail: "status is owned by the bridge transport in Phase 2".to_string(),
        })
    }
}

struct CompatProfileTransport<T>(T);

impl<T> crate::internal::transport::AuthenticatedRpcTransport for CompatProfileTransport<T>
where
    T: BridgeProfileAuthenticatedRpcTransport,
{
    fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value> {
        self.0.authenticated_rpc(endpoint, method, params)
    }
}
