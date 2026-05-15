use super::types::MailError;
use super::wire::ServiceError;
use crate::authsdk::{HttpError, RpcError, Session};
use crate::config::Resolved;
use crate::transportcfg::{new_http_client, HttpClient, Profile};
use serde::de::DeserializeOwned;
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct Client {
    base_url: String,
    http_client: HttpClient,
}

impl Client {
    pub fn new(resolved: &Resolved) -> Result<Self, MailError> {
        if resolved.mail_service_url.trim().is_empty() {
            return Err(MailError::Internal(
                "mail service url is required".to_string(),
            ));
        }
        let http_client = new_http_client(&resolved.ca_bundle)
            .map_err(|err| MailError::Internal(err.to_string()))?;
        Ok(Self {
            base_url: resolved
                .mail_service_url
                .trim()
                .trim_end_matches('/')
                .to_string(),
            http_client,
        })
    }

    pub fn authenticated_rpc_call_profile<T, P>(
        &self,
        _profile: Profile,
        endpoint: &str,
        rpc_method: &str,
        params: P,
        auth: &mut Session,
    ) -> Result<T, MailError>
    where
        T: DeserializeOwned,
        P: Serialize,
    {
        let request_url = format!("{}{}", self.base_url, endpoint);
        auth.do_json_rpc(&self.http_client, &request_url, "POST", rpc_method, params)
            .map_err(mail_service_error)
    }

    pub fn ensure_jwt(&self, auth: &mut Session, request_url: &str) -> Result<String, MailError> {
        auth.ensure_jwt(&self.http_client, request_url)
            .map_err(mail_service_error)
    }
}

fn mail_service_error(err: anyhow::Error) -> MailError {
    match err.downcast::<RpcError>() {
        Ok(err) => return MailError::Service(ServiceError::from(err)),
        Err(err) => match err.downcast::<HttpError>() {
            Ok(err) => MailError::Service(ServiceError::from(err)),
            Err(err) => MailError::Internal(err.to_string()),
        },
    }
}
