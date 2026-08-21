use super::state::AuthStateSnapshot;

pub(crate) trait SessionProvider {
    fn ensure_session(
        &self,
        scope: crate::auth::AuthScope,
    ) -> crate::ImResult<crate::auth::SessionBundle>;

    fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate>;

    fn status(&self) -> crate::ImResult<crate::auth::AuthStatus>;
}

pub(crate) trait AsyncSessionProvider {
    async fn ensure_session(
        &self,
        scope: crate::auth::AuthScope,
    ) -> crate::ImResult<crate::auth::SessionBundle>;

    async fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate>;

    async fn status(&self) -> crate::ImResult<crate::auth::AuthStatus>;
}

pub(crate) struct FileSessionProvider<'a> {
    client: &'a crate::core::ImClient,
}

impl<'a> FileSessionProvider<'a> {
    pub(crate) fn new(client: &'a crate::core::ImClient) -> Self {
        Self { client }
    }

    fn snapshot(&self) -> crate::ImResult<SessionSnapshot> {
        let runtime = self.client.runtime();
        let did_document_available = runtime.key_provider.optional_did_document()?.is_some();
        let private_key_available = runtime.key_provider.request_signing_key_id().is_ok();
        let auth_state = runtime.key_provider.auth_state()?;
        Ok(SessionSnapshot {
            subject: self.client.did().clone(),
            ready_for_auth: self.client.current_identity().readiness.ready_for_auth,
            ready_for_messaging: self.client.current_identity().readiness.ready_for_messaging,
            did_document_available,
            private_key_available,
            auth_state,
        })
    }

    async fn snapshot_async(&self) -> crate::ImResult<SessionSnapshot> {
        self.snapshot()
    }
}

impl SessionProvider for FileSessionProvider<'_> {
    fn ensure_session(
        &self,
        scope: crate::auth::AuthScope,
    ) -> crate::ImResult<crate::auth::SessionBundle> {
        let snapshot = self.snapshot()?;
        snapshot.ensure_ready(scope)?;
        Ok(crate::auth::SessionBundle {
            subject: snapshot.subject,
            scope,
            expires_at: snapshot.auth_state.expires_at.clone(),
            refreshed: false,
            bearer_token: snapshot.auth_state.bearer_token,
        })
    }

    fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
        let snapshot = self.snapshot()?;
        snapshot.ensure_refresh_ready()?;
        let mut transport = crate::internal::transport::CoreHttpTransport::new(self.client);
        transport.refresh_jwt()?;
        let refreshed = self.snapshot()?;
        Ok(crate::auth::SessionUpdate {
            subject: snapshot.subject,
            previous_expires_at: snapshot.auth_state.expires_at.clone(),
            new_expires_at: refreshed.auth_state.expires_at,
            refreshed: true,
            bearer_token: refreshed.auth_state.bearer_token,
        })
    }

    fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
        let snapshot = self.snapshot()?;
        Ok(crate::auth::AuthStatus {
            subject: snapshot.subject.clone(),
            has_session: snapshot.ready_for_auth
                && snapshot.did_document_available
                && snapshot.private_key_available
                && snapshot.auth_state.has_valid_token,
            expires_at: snapshot.auth_state.expires_at.clone(),
            needs_refresh: snapshot.ready_for_auth
                && snapshot.did_document_available
                && snapshot.private_key_available
                && (!snapshot.auth_state.has_token || snapshot.auth_state.needs_refresh),
            warnings: snapshot.warnings(),
        })
    }
}

impl<T> SessionProvider for &T
where
    T: SessionProvider + ?Sized,
{
    fn ensure_session(
        &self,
        scope: crate::auth::AuthScope,
    ) -> crate::ImResult<crate::auth::SessionBundle> {
        (**self).ensure_session(scope)
    }

    fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
        (**self).refresh_session()
    }

    fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
        (**self).status()
    }
}

impl AsyncSessionProvider for FileSessionProvider<'_> {
    async fn ensure_session(
        &self,
        scope: crate::auth::AuthScope,
    ) -> crate::ImResult<crate::auth::SessionBundle> {
        let snapshot = self.snapshot_async().await?;
        snapshot.ensure_identity_ready(scope)?;
        if snapshot.auth_state.has_valid_token && !snapshot.auth_state.needs_refresh {
            return Ok(crate::auth::SessionBundle {
                subject: snapshot.subject,
                scope,
                expires_at: snapshot.auth_state.expires_at.clone(),
                refreshed: false,
                bearer_token: snapshot.auth_state.bearer_token,
            });
        }

        let mut transport = crate::internal::transport::CoreHttpTransport::new(self.client);
        transport.refresh_jwt_async().await?;
        let refreshed = self.snapshot_async().await?;
        refreshed.ensure_ready(scope)?;
        Ok(crate::auth::SessionBundle {
            subject: refreshed.subject,
            scope,
            expires_at: refreshed.auth_state.expires_at.clone(),
            refreshed: true,
            bearer_token: refreshed.auth_state.bearer_token,
        })
    }

    async fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
        let snapshot = self.snapshot_async().await?;
        snapshot.ensure_refresh_ready()?;
        let mut transport = crate::internal::transport::CoreHttpTransport::new(self.client);
        transport.refresh_jwt_async().await?;
        let refreshed = self.snapshot_async().await?;
        Ok(crate::auth::SessionUpdate {
            subject: snapshot.subject,
            previous_expires_at: snapshot.auth_state.expires_at.clone(),
            new_expires_at: refreshed.auth_state.expires_at,
            refreshed: true,
            bearer_token: refreshed.auth_state.bearer_token,
        })
    }

    async fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
        let snapshot = self.snapshot_async().await?;
        Ok(crate::auth::AuthStatus {
            subject: snapshot.subject.clone(),
            has_session: snapshot.ready_for_auth
                && snapshot.did_document_available
                && snapshot.private_key_available
                && snapshot.auth_state.has_valid_token,
            expires_at: snapshot.auth_state.expires_at.clone(),
            needs_refresh: snapshot.ready_for_auth
                && snapshot.did_document_available
                && snapshot.private_key_available
                && (!snapshot.auth_state.has_token || snapshot.auth_state.needs_refresh),
            warnings: snapshot.warnings(),
        })
    }
}

impl<T> AsyncSessionProvider for &T
where
    T: AsyncSessionProvider + ?Sized,
{
    async fn ensure_session(
        &self,
        scope: crate::auth::AuthScope,
    ) -> crate::ImResult<crate::auth::SessionBundle> {
        (**self).ensure_session(scope).await
    }

    async fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
        (**self).refresh_session().await
    }

    async fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
        (**self).status().await
    }
}

struct SessionSnapshot {
    subject: crate::ids::Did,
    ready_for_auth: bool,
    ready_for_messaging: bool,
    did_document_available: bool,
    private_key_available: bool,
    auth_state: AuthStateSnapshot,
}

impl SessionSnapshot {
    fn ensure_ready(&self, scope: crate::auth::AuthScope) -> crate::ImResult<()> {
        self.ensure_identity_ready(scope)?;
        if !self.auth_state.has_token {
            return Err(crate::ImError::AuthRequired);
        }
        if self.auth_state.token_expired {
            return Err(crate::ImError::SessionExpired);
        }
        Ok(())
    }

    fn ensure_identity_ready(&self, scope: crate::auth::AuthScope) -> crate::ImResult<()> {
        if !self.ready_for_auth {
            return Err(crate::ImError::AuthRequired);
        }
        if matches!(
            scope,
            crate::auth::AuthScope::Messaging | crate::auth::AuthScope::GroupMessaging
        ) && !self.ready_for_messaging
        {
            return Err(crate::ImError::IdentityNotReady {
                identity: self.subject.as_str().to_string(),
                missing: vec!["messaging_registration".to_string()],
            });
        }
        if !self.did_document_available {
            return Err(crate::ImError::CredentialFileUnreadable {
                path_kind: "did_document".to_string(),
                detail: "DID document is missing".to_string(),
            });
        }
        if !self.private_key_available {
            return Err(crate::ImError::CredentialFileUnreadable {
                path_kind: "private_key".to_string(),
                detail: "private key material is missing".to_string(),
            });
        }
        Ok(())
    }

    fn warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        if !self.did_document_available {
            warnings.push("did document is missing".to_string());
        }
        if !self.private_key_available {
            warnings.push("private key is missing".to_string());
        }
        if !self.auth_state.has_token {
            warnings.push("auth state has no JWT".to_string());
        } else if self.auth_state.token_expired {
            warnings.push("auth state JWT is expired".to_string());
        } else if self.auth_state.needs_refresh {
            warnings.push("auth state JWT expires soon".to_string());
        }
        warnings
    }

    fn ensure_refresh_ready(&self) -> crate::ImResult<()> {
        if !self.ready_for_auth {
            return Err(crate::ImError::AuthRequired);
        }
        if !self.did_document_available {
            return Err(crate::ImError::CredentialFileUnreadable {
                path_kind: "did_document".to_string(),
                detail: "DID document is missing".to_string(),
            });
        }
        if !self.private_key_available {
            return Err(crate::ImError::CredentialFileUnreadable {
                path_kind: "private_key".to_string(),
                detail: "private key material is missing".to_string(),
            });
        }
        Ok(())
    }
}
