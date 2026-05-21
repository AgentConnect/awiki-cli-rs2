pub struct AuthService<'a> {
    client: &'a crate::core::ImClient,
}

impl<'a> AuthService<'a> {
    pub(crate) fn new(client: &'a crate::core::ImClient) -> Self {
        Self { client }
    }

    pub fn login(&self) -> crate::ImResult<super::SessionBundle> {
        Err(crate::ImError::unsupported("auth-login"))
    }

    pub fn ensure_session(&self, scope: super::AuthScope) -> crate::ImResult<super::SessionBundle> {
        Err(crate::ImError::TransportUnavailable {
            detail: format!("auth session wiring for {scope:?} is not available in Phase 1A"),
        })
    }

    pub fn refresh_session(&self) -> crate::ImResult<super::SessionUpdate> {
        Err(crate::ImError::unsupported("auth-refresh"))
    }

    pub fn status(&self) -> crate::ImResult<super::AuthStatus> {
        Ok(super::AuthStatus {
            subject: self.client.did().clone(),
            has_session: false,
            expires_at: None,
            needs_refresh: true,
            warnings: vec!["auth session implementation is not wired in Phase 1A".to_string()],
        })
    }
}
