use crate::internal::auth::session::{AsyncSessionProvider, SessionProvider};

pub struct AuthService<'a> {
    client: &'a crate::core::ImClient,
}

impl<'a> AuthService<'a> {
    pub(crate) fn new(client: &'a crate::core::ImClient) -> Self {
        Self { client }
    }

    pub fn login(&self) -> crate::ImResult<super::SessionBundle> {
        self.ensure_session(super::AuthScope::UserProfile)
    }

    pub async fn login_async(&self) -> crate::ImResult<super::SessionBundle> {
        self.ensure_session_async(super::AuthScope::UserProfile)
            .await
    }

    pub fn ensure_session(&self, scope: super::AuthScope) -> crate::ImResult<super::SessionBundle> {
        SessionProvider::ensure_session(&self.provider(), scope)
    }

    pub async fn ensure_session_async(
        &self,
        scope: super::AuthScope,
    ) -> crate::ImResult<super::SessionBundle> {
        AsyncSessionProvider::ensure_session(&self.provider(), scope).await
    }

    pub fn refresh_session(&self) -> crate::ImResult<super::SessionUpdate> {
        SessionProvider::refresh_session(&self.provider())
    }

    pub async fn refresh_session_async(&self) -> crate::ImResult<super::SessionUpdate> {
        AsyncSessionProvider::refresh_session(&self.provider()).await
    }

    pub fn status(&self) -> crate::ImResult<super::AuthStatus> {
        SessionProvider::status(&self.provider())
    }

    pub async fn status_async(&self) -> crate::ImResult<super::AuthStatus> {
        AsyncSessionProvider::status(&self.provider()).await
    }

    fn provider(&self) -> crate::internal::auth::session::FileSessionProvider<'a> {
        crate::internal::auth::session::FileSessionProvider::new(self.client)
    }
}
