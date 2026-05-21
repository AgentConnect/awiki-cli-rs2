use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthScope {
    UserProfile,
    Messaging,
    GroupMessaging,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionBundle {
    pub subject: crate::ids::Did,
    pub scope: AuthScope,
    pub expires_at: Option<String>,
    pub refreshed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionUpdate {
    pub subject: crate::ids::Did,
    pub previous_expires_at: Option<String>,
    pub new_expires_at: Option<String>,
    pub refreshed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthStatus {
    pub subject: crate::ids::Did,
    pub has_session: bool,
    pub expires_at: Option<String>,
    pub needs_refresh: bool,
    pub warnings: Vec<String>,
}
