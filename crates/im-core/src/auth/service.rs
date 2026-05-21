pub struct AuthService<'a> {
    client: &'a crate::core::ImClient,
}

impl<'a> AuthService<'a> {
    pub(crate) fn new(client: &'a crate::core::ImClient) -> Self {
        Self { client }
    }

    pub fn login(&self) -> crate::ImResult<super::SessionBundle> {
        Ok(super::SessionBundle {
            subject: self.client.did().clone(),
            scope: super::AuthScope::UserProfile,
            expires_at: None,
            refreshed: false,
        })
    }

    pub fn ensure_session(&self, scope: super::AuthScope) -> crate::ImResult<super::SessionBundle> {
        Ok(super::SessionBundle {
            subject: self.client.did().clone(),
            scope,
            expires_at: None,
            refreshed: false,
        })
    }

    pub fn refresh_session(&self) -> crate::ImResult<super::SessionUpdate> {
        Ok(super::SessionUpdate {
            subject: self.client.did().clone(),
            previous_expires_at: None,
            new_expires_at: None,
            refreshed: false,
        })
    }

    pub fn status(&self) -> crate::ImResult<super::AuthStatus> {
        Ok(super::AuthStatus {
            subject: self.client.did().clone(),
            has_session: self.client.current_identity().readiness.ready_for_auth,
            expires_at: None,
            needs_refresh: false,
            warnings: Vec::new(),
        })
    }
}
