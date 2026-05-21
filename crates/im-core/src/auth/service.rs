use crate::internal::auth::session::SessionProvider;

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

    pub fn ensure_session(&self, scope: super::AuthScope) -> crate::ImResult<super::SessionBundle> {
        self.provider().ensure_session(scope)
    }

    pub fn refresh_session(&self) -> crate::ImResult<super::SessionUpdate> {
        self.provider().refresh_session()
    }

    pub fn status(&self) -> crate::ImResult<super::AuthStatus> {
        self.provider().status()
    }

    fn provider(&self) -> crate::internal::auth::session::FileSessionProvider<'a> {
        crate::internal::auth::session::FileSessionProvider::new(self.client)
    }
}
