pub const MODULE_PATH: &str = "github.com/agent-network-protocol/anp/golang";
pub const MODULE_VERSION: &str = "v0.8.7";

pub use anp::authentication::{AuthMode, AuthenticationError, DIDWbaAuthHeader};

pub const AUTH_MODE_HTTP_SIGNATURES: AuthMode = AuthMode::HttpSignatures;
pub const AUTH_MODE_AUTO: AuthMode = AuthMode::Auto;
