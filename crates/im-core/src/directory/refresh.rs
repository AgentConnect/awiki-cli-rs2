use super::{
    DirectoryService, DisplayProfile, DisplayProfileBatchRequest, DisplayProfileRefreshRequest,
};

impl DirectoryService<'_> {
    /// Refresh display data without creating contacts, conversations or identity bindings.
    /// Per-peer network failures retain the local projection and add a stable warning.
    pub async fn refresh_display_profiles_async(
        &self,
        request: DisplayProfileRefreshRequest,
    ) -> crate::ImResult<Vec<DisplayProfile>> {
        if request.peers.len() > 100
            || request
                .peers
                .iter()
                .any(|p| !p.as_str().starts_with("did:"))
        {
            return Err(crate::ImError::invalid_input(
                Some("peers".to_owned()),
                "display refresh requires at most 100 DIDs",
            ));
        }
        #[cfg(feature = "sqlite")]
        {
            use futures_util::{stream, StreamExt};
            let db = self.client.core_inner().local_state_db().await?;
            let owner =
                crate::internal::local_state::owner_scope::OwnerScope::for_client(self.client)?
                    .owner_identity_id;
            let dids = request
                .peers
                .iter()
                .map(|p| crate::ids::Did::parse(p.as_str()))
                .collect::<crate::ImResult<Vec<_>>>()?;
            let unique = dids
                .iter()
                .map(|d| d.as_str().to_owned())
                .collect::<std::collections::BTreeSet<_>>();
            let outcomes = stream::iter(unique)
                .map(|value| {
                    let db = db.clone();
                    let owner = owner.clone();
                    async move {
                        let did = crate::ids::Did::parse(&value)?;
                        let Some(lease) = db
                            .claim_display_profile_refresh(
                                owner.clone(),
                                did.clone(),
                                request.force,
                            )
                            .await?
                        else {
                            return Ok::<_, crate::ImError>((value, false));
                        };
                        let runtime = crate::internal::directory_runtime::DirectoryRuntime::new(
                            self.client,
                            crate::internal::transport::CoreHttpTransport::new(self.client),
                        );
                        // Use the transport/parser only, deliberately bypassing directory business projection.
                        let result = tokio::time::timeout(
                            std::time::Duration::from_secs(8),
                            runtime.public_profile_async(super::IdentitySubject::Did(did.clone())),
                        )
                        .await;
                        let profile = match result {
                            Ok(Ok(value)) => Some(value.profile),
                            _ => None,
                        };
                        let failed = profile.is_none();
                        db.finish_display_profile_refresh(owner, did, lease, profile)
                            .await?;
                        Ok((value, failed))
                    }
                })
                .buffer_unordered(4)
                .collect::<Vec<_>>()
                .await;
            let failures = outcomes
                .into_iter()
                .collect::<crate::ImResult<Vec<_>>>()?
                .into_iter()
                .filter_map(|(did, failed)| failed.then_some(did))
                .collect::<std::collections::BTreeSet<_>>();
            let mut profiles = self
                .hydrate_display_profiles_async(DisplayProfileBatchRequest {
                    peers: request.peers,
                })
                .await?;
            for profile in &mut profiles {
                if profile
                    .did
                    .as_ref()
                    .is_some_and(|did| failures.contains(did.as_str()))
                {
                    profile
                        .warnings
                        .push("display_profile_refresh_failed".to_owned());
                }
            }
            Ok(profiles)
        }
        #[cfg(not(feature = "sqlite"))]
        {
            let _ = request;
            Err(crate::ImError::unsupported(
                "directory-refresh-display-profiles",
            ))
        }
    }
}
