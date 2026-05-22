#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartAuthScope {
    UserProfile,
    Messaging,
    GroupMessaging,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartAuthStatus {
    pub subject: String,
    pub has_session: bool,
    pub expires_at: Option<String>,
    pub needs_refresh: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartSessionBundle {
    pub subject: String,
    pub scope: DartAuthScope,
    pub expires_at: Option<String>,
    pub refreshed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartSessionUpdate {
    pub subject: String,
    pub previous_expires_at: Option<String>,
    pub new_expires_at: Option<String>,
    pub refreshed: bool,
}
