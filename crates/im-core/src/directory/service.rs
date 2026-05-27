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

fn validate_relationship_list_query(query: &super::RelationshipListQuery) -> crate::ImResult<()> {
    if query.limit.is_some_and(|limit| limit.0 == 0) {
        return Err(crate::ImError::invalid_input(
            Some("limit".to_string()),
            "limit must be greater than zero",
        ));
    }
    Ok(())
}
