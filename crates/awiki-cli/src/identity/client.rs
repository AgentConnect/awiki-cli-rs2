use super::types::IdentityError;
use super::wire::ServiceError;
use crate::authsdk::{
    build_json_rpc_payload, decode_json_rpc_response, http_status_error, HttpError, RpcError,
    Session, CONTENT_TYPE_JSON,
};
use crate::config::{join_base_url, Resolved};
use crate::transportcfg::{new_http_client, HttpClient, HttpRequest, Profile};
use serde::de::DeserializeOwned;
use serde_json::Value;

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

    pub fn rpc_call_profile<T>(
        &self,
        _profile: Profile,
        endpoint: &str,
        rpc_method: &str,
        params: Value,
    ) -> Result<T, IdentityError>
    where
        T: DeserializeOwned,
    {
        let request_url = join_base_url(&self.base_url, endpoint);
        let payload = build_json_rpc_payload(rpc_method, params);
        let body = serde_json::to_vec(&payload)?;
        let response = self
            .http_client
            .execute(
                HttpRequest::new("POST", request_url)
                    .header("Content-Type", CONTENT_TYPE_JSON)
                    .body(body),
            )
            .map_err(|err| IdentityError::Internal(err.to_string()))?;
        if let Some(err) = http_status_error(response.status_code, &response.body) {
            return Err(service_error(err).into());
        }
        decode_json_rpc_response(&response.body).map_err(identity_service_error)
    }

    pub fn ensure_jwt(
        &self,
        auth: &mut Session,
        request_url: &str,
    ) -> Result<String, IdentityError> {
        auth.ensure_jwt(&self.http_client, request_url)
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

fn service_error(err: HttpError) -> ServiceError {
    ServiceError::from(err)
}
