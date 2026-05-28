pub struct DirectoryService<'a> {
    client: &'a crate::core::ImClient,
}

impl<'a> DirectoryService<'a> {
    pub(crate) fn new(client: &'a crate::core::ImClient) -> Self {
        Self { client }
    }

    pub fn resolve_peer(
        &self,
        peer: crate::ids::PeerRef,
    ) -> crate::ImResult<super::DirectoryResolution> {
        if peer.as_str().trim().is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("peer".to_string()),
                "peer must not be empty",
            ));
        }
        self.resolve_peer_with_runtime(
            peer,
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .map(|result| result.resolution)
    }

    pub async fn resolve_peer_async(
        &self,
        peer: crate::ids::PeerRef,
    ) -> crate::ImResult<super::DirectoryResolution> {
        if peer.as_str().trim().is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("peer".to_string()),
                "peer must not be empty",
            ));
        }
        self.resolve_peer_with_runtime_async(
            peer,
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .await
        .map(|result| result.resolution)
    }

    pub(crate) fn resolve_peer_with_runtime<T>(
        &self,
        peer: crate::ids::PeerRef,
        transport: T,
    ) -> crate::ImResult<crate::internal::directory_runtime::DirectoryResolveResult>
    where
        T: crate::internal::transport::RpcTransport,
    {
        let result =
            crate::internal::directory_runtime::DirectoryRuntime::new(self.client, transport)
                .resolve_peer(peer)?;
        #[cfg(feature = "sqlite")]
        crate::internal::contact_store::projection::project_directory_resolution(
            self.client,
            &result.resolution,
        );
        Ok(result)
    }

    pub(crate) async fn resolve_peer_with_runtime_async<T>(
        &self,
        peer: crate::ids::PeerRef,
        transport: T,
    ) -> crate::ImResult<crate::internal::directory_runtime::DirectoryResolveResult>
    where
        T: crate::internal::transport::AsyncRpcTransport,
    {
        let result =
            crate::internal::directory_runtime::DirectoryRuntime::new(self.client, transport)
                .resolve_peer_async(peer)
                .await?;
        #[cfg(feature = "sqlite")]
        crate::internal::contact_store::projection::project_directory_resolution_async(
            self.client,
            &result.resolution,
        )
        .await?;
        Ok(result)
    }

    pub fn lookup_handle(
        &self,
        handle: crate::ids::Handle,
    ) -> crate::ImResult<super::HandleLookupResult> {
        if handle.as_str().trim().is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("handle".to_string()),
                "handle must not be empty",
            ));
        }
        self.lookup_handle_with_runtime(
            handle,
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
    }

    pub async fn lookup_handle_async(
        &self,
        handle: crate::ids::Handle,
    ) -> crate::ImResult<super::HandleLookupResult> {
        if handle.as_str().trim().is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("handle".to_string()),
                "handle must not be empty",
            ));
        }
        self.lookup_handle_with_runtime_async(
            handle,
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .await
    }

    pub(crate) fn lookup_handle_with_runtime<T>(
        &self,
        handle: crate::ids::Handle,
        transport: T,
    ) -> crate::ImResult<super::HandleLookupResult>
    where
        T: crate::internal::transport::RpcTransport,
    {
        crate::internal::directory_runtime::DirectoryRuntime::new(self.client, transport)
            .lookup_handle(handle)
    }

    pub(crate) async fn lookup_handle_with_runtime_async<T>(
        &self,
        handle: crate::ids::Handle,
        transport: T,
    ) -> crate::ImResult<super::HandleLookupResult>
    where
        T: crate::internal::transport::AsyncRpcTransport,
    {
        crate::internal::directory_runtime::DirectoryRuntime::new(self.client, transport)
            .lookup_handle_async(handle)
            .await
    }

    pub fn public_profile(
        &self,
        subject: super::IdentitySubject,
    ) -> crate::ImResult<super::PublicProfile> {
        validate_identity_subject(&subject)?;
        self.public_profile_with_runtime(
            subject,
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
    }

    pub async fn public_profile_async(
        &self,
        subject: super::IdentitySubject,
    ) -> crate::ImResult<super::PublicProfile> {
        validate_identity_subject(&subject)?;
        self.public_profile_with_runtime_async(
            subject,
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .await
    }

    pub(crate) fn public_profile_with_runtime<T>(
        &self,
        subject: super::IdentitySubject,
        transport: T,
    ) -> crate::ImResult<super::PublicProfile>
    where
        T: crate::internal::transport::RpcTransport,
    {
        let result =
            crate::internal::directory_runtime::DirectoryRuntime::new(self.client, transport)
                .public_profile(subject)?;
        #[cfg(feature = "sqlite")]
        crate::internal::contact_store::projection::project_directory_resolution(
            self.client,
            &crate::directory::DirectoryResolution {
                input: result.did.as_str().to_string(),
                did: result.did.clone(),
                handle: result.handle.clone(),
                profile: Some(result.profile.clone()),
                warnings: result.warnings.clone(),
            },
        );
        Ok(result)
    }

    pub(crate) async fn public_profile_with_runtime_async<T>(
        &self,
        subject: super::IdentitySubject,
        transport: T,
    ) -> crate::ImResult<super::PublicProfile>
    where
        T: crate::internal::transport::AsyncRpcTransport,
    {
        let result =
            crate::internal::directory_runtime::DirectoryRuntime::new(self.client, transport)
                .public_profile_async(subject)
                .await?;
        #[cfg(feature = "sqlite")]
        crate::internal::contact_store::projection::project_directory_resolution_async(
            self.client,
            &crate::directory::DirectoryResolution {
                input: result.did.as_str().to_string(),
                did: result.did.clone(),
                handle: result.handle.clone(),
                profile: Some(result.profile.clone()),
                warnings: result.warnings.clone(),
            },
        )
        .await?;
        Ok(result)
    }

    pub fn save_contact(
        &self,
        request: super::SaveContactRequest,
    ) -> crate::ImResult<super::Contact> {
        validate_save_contact(&request)?;
        let (did, handle) = contact_target_from_request(self, &request)?;
        let mut request = request;
        if request.did.is_none() {
            request.did = Some(did.clone());
        }
        if request.handle.is_none() {
            request.handle = handle;
        }
        #[cfg(feature = "sqlite")]
        {
            let mut connection = crate::internal::contact_store::open_writable(self.client)?;
            let record = crate::internal::contact_store::projection::record_from_save_request(
                self.client,
                &request,
                did.clone(),
            );
            crate::internal::contact_store::records::upsert_contact(&mut connection, record)?;
            let record = crate::internal::contact_store::records::get_contact_by_did(
                &connection,
                self.owner_identity_id(),
                self.owner_did().as_str(),
                request
                    .did
                    .as_ref()
                    .map_or_else(|| request.peer.as_str(), crate::ids::Did::as_str),
            )?;
            crate::internal::contact_store::records::contact_to_dto(&record)
        }
        #[cfg(not(feature = "sqlite"))]
        {
            let _ = did;
            Err(crate::ImError::unsupported("directory-save-contact"))
        }
    }

    pub async fn save_contact_async(
        &self,
        request: super::SaveContactRequest,
    ) -> crate::ImResult<super::Contact> {
        validate_save_contact(&request)?;
        let (did, handle) = contact_target_from_request_async(self, &request).await?;
        let mut request = request;
        if request.did.is_none() {
            request.did = Some(did.clone());
        }
        if request.handle.is_none() {
            request.handle = handle;
        }
        #[cfg(feature = "sqlite")]
        {
            let record = crate::internal::contact_store::projection::record_from_save_request(
                self.client,
                &request,
                did.clone(),
            );
            let db = self.client.core_inner().local_state_db().await?;
            db.upsert_contact(record).await?;
            let record = db
                .get_contact_by_did(
                    self.owner_identity_id(),
                    self.owner_did().as_str(),
                    request
                        .did
                        .as_ref()
                        .map_or_else(|| request.peer.as_str(), crate::ids::Did::as_str),
                )
                .await?;
            crate::internal::contact_store::records::contact_to_dto(&record)
        }
        #[cfg(not(feature = "sqlite"))]
        {
            let _ = did;
            Err(crate::ImError::unsupported("directory-save-contact"))
        }
    }

    pub fn contacts(
        &self,
        query: super::ContactListQuery,
    ) -> crate::ImResult<crate::ids::Page<super::Contact>> {
        if query.limit.is_some_and(|limit| limit.0 == 0) {
            return Err(crate::ImError::invalid_input(
                Some("limit".to_string()),
                "limit must be greater than zero",
            ));
        }
        #[cfg(feature = "sqlite")]
        {
            let connection = crate::internal::contact_store::open_writable(self.client)?;
            let limit = query.limit.map(|limit| i64::from(limit.0)).unwrap_or(100);
            let contacts = crate::internal::contact_store::records::list_contacts(
                &connection,
                self.owner_identity_id(),
                self.owner_did().as_str(),
                limit,
            )?;
            let items = contacts
                .iter()
                .map(crate::internal::contact_store::records::contact_to_dto)
                .collect::<crate::ImResult<Vec<_>>>()?;
            Ok(crate::ids::Page {
                items,
                next_cursor: None,
                has_more: false,
            })
        }
        #[cfg(not(feature = "sqlite"))]
        {
            let _ = query;
            Err(crate::ImError::unsupported("directory-contacts"))
        }
    }

    pub async fn contacts_async(
        &self,
        query: super::ContactListQuery,
    ) -> crate::ImResult<crate::ids::Page<super::Contact>> {
        if query.limit.is_some_and(|limit| limit.0 == 0) {
            return Err(crate::ImError::invalid_input(
                Some("limit".to_string()),
                "limit must be greater than zero",
            ));
        }
        #[cfg(feature = "sqlite")]
        {
            let limit = query.limit.map(|limit| i64::from(limit.0)).unwrap_or(100);
            let db = self.client.core_inner().local_state_db().await?;
            let contacts = db
                .list_contacts(self.owner_identity_id(), self.owner_did().as_str(), limit)
                .await?;
            let items = contacts
                .iter()
                .map(crate::internal::contact_store::records::contact_to_dto)
                .collect::<crate::ImResult<Vec<_>>>()?;
            Ok(crate::ids::Page {
                items,
                next_cursor: None,
                has_more: false,
            })
        }
        #[cfg(not(feature = "sqlite"))]
        {
            let _ = query;
            Err(crate::ImError::unsupported("directory-contacts"))
        }
    }

    pub fn relation_status(
        &self,
        peer: crate::ids::PeerRef,
    ) -> crate::ImResult<super::RelationStatus> {
        if peer.as_str().trim().is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("peer".to_string()),
                "peer must not be empty",
            ));
        }
        #[cfg(feature = "sqlite")]
        {
            let connection = crate::internal::contact_store::open_writable(self.client)?;
            let record = if peer.as_str().trim().starts_with("did:") {
                crate::internal::contact_store::records::get_contact_by_did(
                    &connection,
                    self.owner_identity_id(),
                    self.owner_did().as_str(),
                    peer.as_str(),
                )
                .ok()
            } else {
                crate::internal::contact_store::records::get_current_contact_by_handle(
                    &connection,
                    self.owner_identity_id(),
                    self.owner_did().as_str(),
                    peer.as_str(),
                )
                .ok()
            };
            crate::internal::contact_store::records::relation_status_from_record(peer, record)
        }
        #[cfg(not(feature = "sqlite"))]
        {
            let _ = peer;
            Err(crate::ImError::unsupported("directory-relation-status"))
        }
    }

    pub async fn relation_status_async(
        &self,
        peer: crate::ids::PeerRef,
    ) -> crate::ImResult<super::RelationStatus> {
        if peer.as_str().trim().is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("peer".to_string()),
                "peer must not be empty",
            ));
        }
        #[cfg(feature = "sqlite")]
        {
            let db = self.client.core_inner().local_state_db().await?;
            let record = if peer.as_str().trim().starts_with("did:") {
                db.get_contact_by_did(
                    self.owner_identity_id(),
                    self.owner_did().as_str(),
                    peer.as_str(),
                )
                .await
                .ok()
            } else {
                db.get_current_contact_by_handle(
                    self.owner_identity_id(),
                    self.owner_did().as_str(),
                    peer.as_str(),
                )
                .await
                .ok()
            };
            crate::internal::contact_store::records::relation_status_from_record(peer, record)
        }
        #[cfg(not(feature = "sqlite"))]
        {
            let _ = peer;
            Err(crate::ImError::unsupported("directory-relation-status"))
        }
    }

    pub fn owner_did(&self) -> &crate::ids::Did {
        self.client.did()
    }

    pub fn follow(&self, request: super::FollowRequest) -> crate::ImResult<super::FollowResult> {
        validate_peer(request.peer.as_str())?;
        self.follow_with_runtime(
            request,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
    }

    pub async fn follow_async(
        &self,
        request: super::FollowRequest,
    ) -> crate::ImResult<super::FollowResult> {
        validate_peer(request.peer.as_str())?;
        self.follow_with_runtime_async(
            request,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .await
    }

    pub(crate) fn follow_with_runtime<P, T>(
        &self,
        request: super::FollowRequest,
        session_provider: P,
        transport: T,
    ) -> crate::ImResult<super::FollowResult>
    where
        P: crate::internal::auth::session::SessionProvider,
        T: crate::internal::transport::AuthenticatedRpcTransport
            + crate::internal::transport::RpcTransport,
    {
        crate::internal::relationship_runtime::RelationshipRuntime::new(
            self.client,
            session_provider,
            transport,
        )
        .follow(request)
    }

    pub(crate) async fn follow_with_runtime_async<P, T>(
        &self,
        request: super::FollowRequest,
        session_provider: P,
        transport: T,
    ) -> crate::ImResult<super::FollowResult>
    where
        P: crate::internal::auth::session::AsyncSessionProvider,
        T: crate::internal::transport::AsyncAuthenticatedRpcTransport
            + crate::internal::transport::AsyncRpcTransport,
    {
        crate::internal::relationship_runtime::RelationshipRuntime::new(
            self.client,
            session_provider,
            transport,
        )
        .follow_async(request)
        .await
    }

    pub fn unfollow(
        &self,
        request: super::UnfollowRequest,
    ) -> crate::ImResult<super::UnfollowResult> {
        validate_peer(request.peer.as_str())?;
        self.unfollow_with_runtime(
            request,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
    }

    pub async fn unfollow_async(
        &self,
        request: super::UnfollowRequest,
    ) -> crate::ImResult<super::UnfollowResult> {
        validate_peer(request.peer.as_str())?;
        self.unfollow_with_runtime_async(
            request,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .await
    }

    pub(crate) fn unfollow_with_runtime<P, T>(
        &self,
        request: super::UnfollowRequest,
        session_provider: P,
        transport: T,
    ) -> crate::ImResult<super::UnfollowResult>
    where
        P: crate::internal::auth::session::SessionProvider,
        T: crate::internal::transport::AuthenticatedRpcTransport
            + crate::internal::transport::RpcTransport,
    {
        crate::internal::relationship_runtime::RelationshipRuntime::new(
            self.client,
            session_provider,
            transport,
        )
        .unfollow(request)
    }

    pub(crate) async fn unfollow_with_runtime_async<P, T>(
        &self,
        request: super::UnfollowRequest,
        session_provider: P,
        transport: T,
    ) -> crate::ImResult<super::UnfollowResult>
    where
        P: crate::internal::auth::session::AsyncSessionProvider,
        T: crate::internal::transport::AsyncAuthenticatedRpcTransport
            + crate::internal::transport::AsyncRpcTransport,
    {
        crate::internal::relationship_runtime::RelationshipRuntime::new(
            self.client,
            session_provider,
            transport,
        )
        .unfollow_async(request)
        .await
    }

    pub fn relationship_status(
        &self,
        peer: crate::ids::PeerRef,
    ) -> crate::ImResult<super::RelationshipStatus> {
        validate_peer(peer.as_str())?;
        self.relationship_status_with_runtime(
            peer,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
    }

    pub async fn relationship_status_async(
        &self,
        peer: crate::ids::PeerRef,
    ) -> crate::ImResult<super::RelationshipStatus> {
        validate_peer(peer.as_str())?;
        self.relationship_status_with_runtime_async(
            peer,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .await
    }

    pub(crate) fn relationship_status_with_runtime<P, T>(
        &self,
        peer: crate::ids::PeerRef,
        session_provider: P,
        transport: T,
    ) -> crate::ImResult<super::RelationshipStatus>
    where
        P: crate::internal::auth::session::SessionProvider,
        T: crate::internal::transport::AuthenticatedRpcTransport
            + crate::internal::transport::RpcTransport,
    {
        crate::internal::relationship_runtime::RelationshipRuntime::new(
            self.client,
            session_provider,
            transport,
        )
        .relationship_status(peer)
    }

    pub(crate) async fn relationship_status_with_runtime_async<P, T>(
        &self,
        peer: crate::ids::PeerRef,
        session_provider: P,
        transport: T,
    ) -> crate::ImResult<super::RelationshipStatus>
    where
        P: crate::internal::auth::session::AsyncSessionProvider,
        T: crate::internal::transport::AsyncAuthenticatedRpcTransport
            + crate::internal::transport::AsyncRpcTransport,
    {
        crate::internal::relationship_runtime::RelationshipRuntime::new(
            self.client,
            session_provider,
            transport,
        )
        .relationship_status_async(peer)
        .await
    }

    pub fn followers(
        &self,
        query: super::RelationshipListQuery,
    ) -> crate::ImResult<crate::ids::Page<super::RelationshipListItem>> {
        validate_relationship_list_query(&query)?;
        self.followers_with_runtime(
            query,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
    }

    pub async fn followers_async(
        &self,
        query: super::RelationshipListQuery,
    ) -> crate::ImResult<crate::ids::Page<super::RelationshipListItem>> {
        validate_relationship_list_query(&query)?;
        self.followers_with_runtime_async(
            query,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .await
    }

    pub(crate) fn followers_with_runtime<P, T>(
        &self,
        query: super::RelationshipListQuery,
        session_provider: P,
        transport: T,
    ) -> crate::ImResult<crate::ids::Page<super::RelationshipListItem>>
    where
        P: crate::internal::auth::session::SessionProvider,
        T: crate::internal::transport::AuthenticatedRpcTransport
            + crate::internal::transport::RpcTransport,
    {
        crate::internal::relationship_runtime::RelationshipRuntime::new(
            self.client,
            session_provider,
            transport,
        )
        .followers(query)
    }

    pub(crate) async fn followers_with_runtime_async<P, T>(
        &self,
        query: super::RelationshipListQuery,
        session_provider: P,
        transport: T,
    ) -> crate::ImResult<crate::ids::Page<super::RelationshipListItem>>
    where
        P: crate::internal::auth::session::AsyncSessionProvider,
        T: crate::internal::transport::AsyncAuthenticatedRpcTransport
            + crate::internal::transport::AsyncRpcTransport,
    {
        crate::internal::relationship_runtime::RelationshipRuntime::new(
            self.client,
            session_provider,
            transport,
        )
        .followers_async(query)
        .await
    }

    pub fn following(
        &self,
        query: super::RelationshipListQuery,
    ) -> crate::ImResult<crate::ids::Page<super::RelationshipListItem>> {
        validate_relationship_list_query(&query)?;
        self.following_with_runtime(
            query,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
    }

    pub async fn following_async(
        &self,
        query: super::RelationshipListQuery,
    ) -> crate::ImResult<crate::ids::Page<super::RelationshipListItem>> {
        validate_relationship_list_query(&query)?;
        self.following_with_runtime_async(
            query,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .await
    }

    pub(crate) fn following_with_runtime<P, T>(
        &self,
        query: super::RelationshipListQuery,
        session_provider: P,
        transport: T,
    ) -> crate::ImResult<crate::ids::Page<super::RelationshipListItem>>
    where
        P: crate::internal::auth::session::SessionProvider,
        T: crate::internal::transport::AuthenticatedRpcTransport
            + crate::internal::transport::RpcTransport,
    {
        crate::internal::relationship_runtime::RelationshipRuntime::new(
            self.client,
            session_provider,
            transport,
        )
        .following(query)
    }

    pub(crate) async fn following_with_runtime_async<P, T>(
        &self,
        query: super::RelationshipListQuery,
        session_provider: P,
        transport: T,
    ) -> crate::ImResult<crate::ids::Page<super::RelationshipListItem>>
    where
        P: crate::internal::auth::session::AsyncSessionProvider,
        T: crate::internal::transport::AsyncAuthenticatedRpcTransport
            + crate::internal::transport::AsyncRpcTransport,
    {
        crate::internal::relationship_runtime::RelationshipRuntime::new(
            self.client,
            session_provider,
            transport,
        )
        .following_async(query)
        .await
    }

    fn owner_identity_id(&self) -> &str {
        self.client.current_identity().id.as_str()
    }
}

fn validate_peer(peer: &str) -> crate::ImResult<()> {
    if peer.trim().is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("peer".to_string()),
            "peer must not be empty",
        ));
    }
    Ok(())
}

fn validate_identity_subject(subject: &super::IdentitySubject) -> crate::ImResult<()> {
    match subject {
        super::IdentitySubject::Did(did) if did.as_str().trim().is_empty() => Err(
            crate::ImError::invalid_input(Some("did".to_string()), "did must not be empty"),
        ),
        super::IdentitySubject::Handle(handle) if handle.as_str().trim().is_empty() => Err(
            crate::ImError::invalid_input(Some("handle".to_string()), "handle must not be empty"),
        ),
        super::IdentitySubject::Any(value) if value.trim().is_empty() => Err(
            crate::ImError::invalid_input(Some("subject".to_string()), "subject must not be empty"),
        ),
        _ => Ok(()),
    }
}

fn validate_save_contact(request: &super::SaveContactRequest) -> crate::ImResult<()> {
    if request.peer.as_str().trim().is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("peer".to_string()),
            "peer must not be empty",
        ));
    }
    if request
        .relationship
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(crate::ImError::invalid_input(
            Some("relationship".to_string()),
            "relationship must not be empty when provided",
        ));
    }
    Ok(())
}

fn contact_target_from_request(
    service: &DirectoryService<'_>,
    request: &super::SaveContactRequest,
) -> crate::ImResult<(crate::ids::Did, Option<crate::ids::Handle>)> {
    if let Some(did) = &request.did {
        return Ok((did.clone(), request.handle.clone()));
    }
    if request.peer.as_str().starts_with("did:") {
        return crate::ids::Did::parse(request.peer.as_str())
            .map(|did| (did, request.handle.clone()));
    }
    let resolved = service.resolve_peer(request.peer.clone())?;
    Ok((resolved.did, request.handle.clone().or(resolved.handle)))
}

async fn contact_target_from_request_async(
    service: &DirectoryService<'_>,
    request: &super::SaveContactRequest,
) -> crate::ImResult<(crate::ids::Did, Option<crate::ids::Handle>)> {
    if let Some(did) = &request.did {
        return Ok((did.clone(), request.handle.clone()));
    }
    if request.peer.as_str().starts_with("did:") {
        return crate::ids::Did::parse(request.peer.as_str())
            .map(|did| (did, request.handle.clone()));
    }
    let resolved = service.resolve_peer_async(request.peer.clone()).await?;
    Ok((resolved.did, request.handle.clone().or(resolved.handle)))
}

fn validate_relationship_list_query(query: &super::RelationshipListQuery) -> crate::ImResult<()> {
    if query.limit.is_some_and(|limit| limit.0 == 0) {
        return Err(crate::ImError::invalid_input(
            Some("limit".to_string()),
            "limit must be greater than zero",
        ));
    }
    Ok(())
}
