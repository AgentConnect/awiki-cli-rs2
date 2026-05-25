use super::types::IdentityError;
use super::wire::{RestCall, ServiceError};
use crate::authsdk::{
    build_json_rpc_payload, decode_json_rpc_response, decode_plain_json_response,
    http_status_error, HttpError, RpcError, Session, CONTENT_TYPE_JSON,
};
use crate::config::{join_base_url, Resolved};
use crate::traceutil;
use crate::transportcfg::{new_http_client, HttpClient, HttpRequest, Profile};
use serde::de::DeserializeOwned;
use serde::Serialize;
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
        profile: Profile,
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
        let mut phase = traceutil::rpc_phase(rpc_method);
        let response = self
            .http_client
            .execute(
                HttpRequest::new("POST", request_url)
                    .header("Content-Type", CONTENT_TYPE_JSON)
                    .timeout(self.http_client.config().timeout_for_profile(profile))
                    .body(body),
            )
            .map_err(|err| IdentityError::Internal(err.to_string()));
        phase.finish();
        let response = response?;
        if let Some(err) = http_status_error(response.status_code, &response.body) {
            return Err(service_error(err).into());
        }
        decode_json_rpc_response(&response.body).map_err(identity_service_error)
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

    pub fn authenticated_rest_post<T>(
        &self,
        call: RestCall,
        auth: &mut Session,
    ) -> Result<T, IdentityError>
    where
        T: DeserializeOwned,
    {
        let request_url = join_base_url(&self.base_url, call.endpoint);
        auth.do_json_profile_traced(
            &self.http_client,
            Profile::RpcDefault,
            call.method,
            &request_url,
            call.body,
            &format!("{} {}", call.method, call.endpoint),
        )
        .map_err(identity_service_error)
    }

    pub fn rest_post<T>(&self, call: RestCall) -> Result<T, IdentityError>
    where
        T: DeserializeOwned,
    {
        let request_url = join_base_url(&self.base_url, call.endpoint);
        let body = serde_json::to_vec(&call.body)?;
        let mut phase = traceutil::rpc_phase(&format!("{} {}", call.method, call.endpoint));
        let response = self
            .http_client
            .execute(
                HttpRequest::new(call.method, request_url)
                    .header("Content-Type", CONTENT_TYPE_JSON)
                    .timeout(
                        self.http_client
                            .config()
                            .timeout_for_profile(Profile::RpcDefault),
                    )
                    .body(body),
            )
            .map_err(|err| IdentityError::Internal(err.to_string()));
        phase.finish();
        let response = response?;
        if let Some(err) = http_status_error(response.status_code, &response.body) {
            return Err(service_error(err).into());
        }
        decode_plain_json_response(&response.body).map_err(IdentityError::from)
    }

    pub fn rest_get_with_bearer<T>(&self, call: RestCall, bearer: &str) -> Result<T, IdentityError>
    where
        T: DeserializeOwned,
    {
        let request_url = append_query(&join_base_url(&self.base_url, call.endpoint), &call.query);
        let mut request = HttpRequest::new(call.method, request_url);
        let bearer = bearer.trim();
        if !bearer.is_empty() {
            request = request.header("Authorization", format!("Bearer {bearer}"));
        }
        request = request.timeout(
            self.http_client
                .config()
                .timeout_for_profile(Profile::RpcDefault),
        );
        let mut phase = traceutil::rpc_phase(&format!("{} {}", call.method, call.endpoint));
        let response = self
            .http_client
            .execute(request)
            .map_err(|err| IdentityError::Internal(err.to_string()));
        phase.finish();
        let response = response?;
        if let Some(err) = http_status_error(response.status_code, &response.body) {
            return Err(service_error(err).into());
        }
        decode_plain_json_response(&response.body).map_err(IdentityError::from)
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

fn service_error(err: HttpError) -> ServiceError {
    ServiceError::from(err)
}

fn append_query(base_url: &str, query: &std::collections::BTreeMap<String, String>) -> String {
    if query.is_empty() {
        return base_url.to_string();
    }
    let separator = if base_url.contains('?') { '&' } else { '?' };
    let pairs = query
        .iter()
        .map(|(key, value)| format!("{}={}", percent_encode(key), percent_encode(value)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{base_url}{separator}{pairs}")
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}
