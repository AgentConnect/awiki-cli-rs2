use super::auth::{HttpError, RpcError};
use crate::cli_http::Profile;
use serde_json::Value;
use std::fmt;

pub const DID_AUTH_RPC_ENDPOINT: &str = "/user-service/v1/did-auth/rpc";

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
