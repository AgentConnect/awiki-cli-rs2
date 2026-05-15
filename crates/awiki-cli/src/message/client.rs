use super::types::MessageError;
use crate::authsdk::{HttpError, RpcError, Session};
use crate::config::{join_base_url, Resolved};
use crate::identity::wire::ServiceError;
use crate::identity::wire::DID_AUTH_RPC_ENDPOINT;
use crate::transportcfg::{new_http_client, HttpClient, Profile};
use serde::de::DeserializeOwned;
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct Client {
    base_url: String,
    http_client: HttpClient,
}

impl Client {
    pub fn new(resolved: &Resolved) -> Result<Self, MessageError> {
        if resolved.service_base_url.trim().is_empty() {
            return Err(MessageError::Internal(
                "message service url is required".to_string(),
            ));
        }
        let http_client = new_http_client(&resolved.ca_bundle)
            .map_err(|err| MessageError::Internal(err.to_string()))?;
        Ok(Self {
            base_url: resolved
                .service_base_url
                .trim()
                .trim_end_matches('/')
                .to_string(),
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
    ) -> Result<T, MessageError>
    where
        T: DeserializeOwned,
        P: Serialize + Clone,
    {
        let request_url = join_base_url(&self.base_url, endpoint);
        authenticated_rpc_call_url(
            &self.http_client,
            &self.base_url,
            &request_url,
            profile,
            rpc_method,
            params,
            auth,
        )
    }

    pub fn ensure_jwt(
        &self,
        auth: &mut Session,
        request_url: &str,
    ) -> Result<String, MessageError> {
        auth.ensure_jwt_profile(&self.http_client, Profile::AuthRefresh, request_url)
            .map_err(message_service_error)
    }
}

pub(crate) fn authenticated_rpc_call_url<T, P>(
    http_client: &HttpClient,
    base_url: &str,
    request_url: &str,
    profile: Profile,
    rpc_method: &str,
    params: P,
    auth: &mut Session,
) -> Result<T, MessageError>
where
    T: DeserializeOwned,
    P: Serialize + Clone,
{
    match auth.do_json_rpc_profile(
        http_client,
        profile,
        request_url,
        "POST",
        rpc_method,
        params.clone(),
    ) {
        Ok(result) => Ok(result),
        Err(err) => match err.downcast::<RpcError>() {
            Ok(rpc_err) if rpc_err.code == 1401 => {
                let did_auth_url = join_base_url(base_url, DID_AUTH_RPC_ENDPOINT);
                match auth.ensure_jwt_profile(http_client, Profile::AuthRefresh, &did_auth_url) {
                    Ok(_) => auth
                        .do_json_rpc_profile(
                            http_client,
                            profile,
                            request_url,
                            "POST",
                            rpc_method,
                            params,
                        )
                        .map_err(message_service_error),
                    Err(_) => Err(MessageError::Service(ServiceError::from(rpc_err))),
                }
            }
            Ok(rpc_err) => Err(MessageError::Service(ServiceError::from(rpc_err))),
            Err(err) => Err(message_service_error(err)),
        },
    }
}

pub(crate) fn message_service_error(err: anyhow::Error) -> MessageError {
    match err.downcast::<RpcError>() {
        Ok(err) => MessageError::Service(ServiceError::from(err)),
        Err(err) => match err.downcast::<HttpError>() {
            Ok(err) => MessageError::Service(ServiceError::from(err)),
            Err(err) => MessageError::Internal(err.to_string()),
        },
    }
}
