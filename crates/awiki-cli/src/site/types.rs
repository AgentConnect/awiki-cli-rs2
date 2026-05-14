use serde_json::Value;
use std::fmt;

pub const SITE_RPC_ENDPOINT: &str = "/site/rpc";
pub const DID_AUTH_RPC_ENDPOINT: &str = "/user-service/did-auth/rpc";

#[derive(Debug, Clone, PartialEq)]
pub struct CommandResult {
    pub data: Value,
    pub summary: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SiteError {
    DomainRequired,
    DomainInvalid(String),
    SlugRequired,
    NoBodySourceProvided,
    BodySourceConflict,
}

impl fmt::Display for SiteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DomainRequired => formatter.write_str("domain is required"),
            Self::DomainInvalid(message) => formatter.write_str(message),
            Self::SlugRequired => formatter.write_str("slug is required"),
            Self::NoBodySourceProvided => {
                formatter.write_str("provide either inline markdown or markdown file")
            }
            Self::BodySourceConflict => {
                formatter.write_str("use either inline markdown or markdown file, not both")
            }
        }
    }
}

impl std::error::Error for SiteError {}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SetRootParams {
    pub domain: String,
    pub body: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CreatePageParams {
    pub domain: String,
    pub slug: String,
    pub body: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpdatePageParams {
    pub domain: String,
    pub slug: String,
    pub body: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenamePageParams {
    pub domain: String,
    pub slug: String,
    pub to: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IdentitySummary {
    pub identity_name: String,
    pub did: String,
    pub handle: String,
}
