mod dto;
mod service;

#[cfg(test)]
mod tests;

pub use dto::{
    ExternalHttpAuthAttempt, ExternalHttpAuthDecision, ExternalHttpHeader, ExternalHttpRequest,
    ExternalHttpResponse, EXTERNAL_HTTP_AUTH_MAX_BODY_BYTES,
};
pub use service::ExternalHttpAuthService;

pub(crate) use service::ExternalHttpAuthState;
