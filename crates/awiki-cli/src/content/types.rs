use serde_json::Value;
use std::fmt;

pub const CONTENT_RPC_ENDPOINT: &str = "/content/rpc";
pub const DID_AUTH_RPC_ENDPOINT: &str = "/user-service/did-auth/rpc";

#[derive(Debug, Clone)]
pub struct CommandResult {
    pub data: Value,
    pub summary: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentError {
    SlugRequired,
    TitleRequired,
    BodySourceConflict,
    NoUpdateFields,
    VisibilityInvalid,
    AuthIdentityRequired,
}

impl fmt::Display for ContentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SlugRequired => formatter.write_str("slug is required"),
            Self::TitleRequired => formatter.write_str("title is required"),
            Self::BodySourceConflict => {
                formatter.write_str("use either inline markdown or markdown file, not both")
            }
            Self::NoUpdateFields => formatter.write_str("no update fields were provided"),
            Self::VisibilityInvalid => {
                formatter.write_str("visibility must be one of public, draft, or unlisted")
            }
            Self::AuthIdentityRequired => formatter.write_str("active identity is required"),
        }
    }
}

impl std::error::Error for ContentError {}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CreatePageParams {
    pub slug: String,
    pub title: String,
    pub body: String,
    pub visibility: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpdatePageParams {
    pub slug: String,
    pub title: String,
    pub body: Option<String>,
    pub visibility: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenamePageParams {
    pub slug: String,
    pub to: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IdentitySummary {
    pub identity_name: String,
    pub did: String,
    pub handle: String,
}
