use super::auth::{HttpError, RpcError, Session};
use super::types::IdentityError;
use super::wire::ServiceError;
use crate::cli_http::{new_http_client, HttpClient, Profile};
use crate::workspace_config::{join_base_url, Resolved};
use serde::de::DeserializeOwned;
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct Client {
    base_url: String,
    http_client: HttpClient,
}

impl Client {
    pub fn new(resolved: &Resolved) -> Result<Self, IdentityError> {
        if resolved.service_base_url.trim().is_empty() {
            return Err(IdentityError::InvalidInput(
                "service base url is required".to_string(),
            ));
        }
        let http_client = new_http_client(&resolved.ca_bundle)
            .map_err(|err| IdentityError::Internal(err.to_string()))?;
        Ok(Self {
            base_url: resolved.service_base_url.clone(),
            http_client,
        })
    }

    pub fn authenticated_rpc_call_profile<T, P>(
        &self,
        profile: Profile,
        endpoint: &str,
        rpc_method: &str,
        params: P,
        auth: &mut Session,
    ) -> Result<T, IdentityError>
    where
        T: DeserializeOwned,
        P: Serialize,
    {
        let request_url = join_base_url(&self.base_url, endpoint);
        auth.do_json_rpc_profile(
            &self.http_client,
            profile,
            &request_url,
            "POST",
            rpc_method,
            params,
        )
        .map_err(identity_service_error)
    }

    pub fn ensure_jwt(
        &self,
        auth: &mut Session,
        request_url: &str,
        trace_operation: &str,
    ) -> Result<String, IdentityError> {
        auth.ensure_jwt_profile_traced(
            &self.http_client,
            Profile::AuthRefresh,
            request_url,
            trace_operation,
        )
        .map_err(identity_service_error)
    }
}

fn identity_service_error(err: anyhow::Error) -> IdentityError {
    match err.downcast::<RpcError>() {
        Ok(err) => IdentityError::Service(ServiceError::from(err)),
        Err(err) => match err.downcast::<HttpError>() {
            Ok(err) => IdentityError::Service(ServiceError::from(err)),
            Err(err) => IdentityError::Internal(err.to_string()),
        },
    }
}
