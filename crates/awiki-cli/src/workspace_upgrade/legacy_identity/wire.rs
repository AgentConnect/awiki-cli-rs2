use super::auth::{HttpError, RpcError};
use super::service::CommandResult;
use super::types::IdentitySummary;
use crate::cli_http::Profile;
use serde_json::{json, Value};
use std::fmt;

pub const DID_AUTH_RPC_ENDPOINT: &str = "/user-service/did-auth/rpc";

#[derive(Debug, Clone, PartialEq)]
pub struct RpcCall {
    pub endpoint: &'static str,
    pub method: &'static str,
    pub profile: Profile,
    pub params: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ServiceError {
    pub status_code: u16,
    pub rpc_code: i64,
    pub message: String,
    pub data: Option<Value>,
}

impl fmt::Display for ServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.rpc_code, self.status_code) {
            (code, _) if code != 0 => {
                write!(formatter, "service rpc error {code}: {}", self.message)
            }
            (_, status_code) if status_code != 0 => {
                write!(
                    formatter,
                    "service http error {status_code}: {}",
                    self.message
                )
            }
            _ => formatter.write_str(&self.message),
        }
    }
}

impl std::error::Error for ServiceError {}

impl From<RpcError> for ServiceError {
    fn from(value: RpcError) -> Self {
        Self {
            status_code: 0,
            rpc_code: value.code,
            message: value.message,
            data: value.data,
        }
    }
}

impl From<HttpError> for ServiceError {
    fn from(value: HttpError) -> Self {
        Self {
            status_code: value.status_code,
            rpc_code: 0,
            message: value.message,
            data: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReplaceDidRpcParams {
    pub new_did_document: Value,
    pub is_public: Option<bool>,
    pub is_agent: Option<bool>,
    pub role: Option<String>,
    pub endpoint_url: Option<String>,
}

pub fn build_replace_did_rpc_call(params: ReplaceDidRpcParams) -> RpcCall {
    legacy_rpc_call(im_core::compat::identity::build_replace_did_rpc_call(
        im_core::compat::identity::ReplaceDidRpcParams {
            new_did_document: params.new_did_document,
            is_public: params.is_public,
            is_agent: params.is_agent,
            role: params.role,
            endpoint_url: params.endpoint_url,
        },
    ))
}

pub fn replace_did_result(
    identity: &IdentitySummary,
    old_did: &str,
    did: &str,
    backup_path: &str,
    result: Value,
) -> CommandResult {
    CommandResult {
        data: json!({
            "action": "replace_did",
            "identity": identity,
            "old_did": old_did,
            "did": did,
            "backup_path": backup_path,
            "result": result,
        }),
        summary: format!(
            "Identity {} DID replaced successfully",
            identity.identity_name
        ),
        warnings: Vec::new(),
    }
}

fn legacy_rpc_call(call: im_core::compat::identity::RpcCall) -> RpcCall {
    RpcCall {
        endpoint: call.endpoint,
        method: call.method,
        profile: legacy_transport_profile(call.profile),
        params: call.params,
    }
}

fn legacy_transport_profile(profile: im_core::compat::identity::TransportProfile) -> Profile {
    match profile {
        im_core::compat::identity::TransportProfile::BridgeFastPath => Profile::BridgeFastPath,
        im_core::compat::identity::TransportProfile::HealthProbe => Profile::HealthProbe,
        im_core::compat::identity::TransportProfile::AuthRefresh => Profile::AuthRefresh,
        im_core::compat::identity::TransportProfile::RpcDefault => Profile::RpcDefault,
        im_core::compat::identity::TransportProfile::RpcReadHeavy => Profile::RpcReadHeavy,
    }
}
