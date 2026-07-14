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
        if let Some(lookup) = result.handle_lookup.as_ref() {
            project_handle_lookup(self.client, lookup)?;
        } else {
            crate::internal::contact_store::projection::project_directory_resolution(
                self.client,
                &result.resolution,
            );
        }
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
        if let Some(lookup) = result.handle_lookup.as_ref() {
            project_handle_lookup_async(self.client, lookup).await?;
        } else {
            crate::internal::contact_store::projection::project_directory_resolution_async(
                self.client,
                &result.resolution,
            )
            .await?;
        }
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
        let result =
            crate::internal::directory_runtime::DirectoryRuntime::new(self.client, transport)
                .lookup_handle(handle)?;
        #[cfg(feature = "sqlite")]
        project_handle_lookup(self.client, &result)?;
        Ok(result)
    }

    pub(crate) async fn lookup_handle_with_runtime_async<T>(
        &self,
        handle: crate::ids::Handle,
        transport: T,
    ) -> crate::ImResult<super::HandleLookupResult>
    where
        T: crate::internal::transport::AsyncRpcTransport,
    {
        let result =
            crate::internal::directory_runtime::DirectoryRuntime::new(self.client, transport)
                .lookup_handle_async(handle)
                .await?;
        #[cfg(feature = "sqlite")]
        project_handle_lookup_async(self.client, &result).await?;
        Ok(result)
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
                conversation_id: crate::internal::local_state::owner_scope::direct_conversation_id(
                    result.did.as_str(),
                ),
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
                conversation_id: crate::internal::local_state::owner_scope::direct_conversation_id(
                    result.did.as_str(),
                ),
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
        #[cfg(all(feature = "sqlite", feature = "blocking"))]
        {
            let mut connection = crate::internal::contact_store::open_writable(self.client)?;
            let record = crate::internal::contact_store::projection::record_from_save_request(
                self.client,
                &request,
                did.clone(),
            )?;
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
        #[cfg(all(feature = "sqlite", not(feature = "blocking")))]
        {
            let _ = did;
            Err(crate::ImError::unsupported("sync-directory-save-contact"))
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
            )?;
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
        #[cfg(all(feature = "sqlite", feature = "blocking"))]
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
        #[cfg(all(feature = "sqlite", not(feature = "blocking")))]
        {
            let _ = query;
            Err(crate::ImError::unsupported("sync-directory-contacts"))
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

    pub fn hydrate_display_profiles(
        &self,
        request: super::DisplayProfileBatchRequest,
    ) -> crate::ImResult<Vec<super::DisplayProfile>> {
        validate_display_profile_batch_request(&request)?;
        #[cfg(all(feature = "sqlite", feature = "blocking"))]
        {
            let connection = crate::internal::contact_store::open_writable(self.client)?;
            request
                .peers
                .into_iter()
                .map(|peer| {
                    let record = local_contact_for_peer(
                        |did| {
                            crate::internal::contact_store::records::get_contact_by_did(
                                &connection,
                                self.owner_identity_id(),
                                self.owner_did().as_str(),
                                did,
                            )
                        },
                        |handle| {
                            crate::internal::contact_store::records::get_current_contact_by_handle(
                                &connection,
                                self.owner_identity_id(),
                                self.owner_did().as_str(),
                                handle,
                            )
                        },
                        &peer,
                    );
                    display_profile_from_local_result(peer, record)
                })
                .collect()
        }
        #[cfg(all(feature = "sqlite", not(feature = "blocking")))]
        {
            let _ = request;
            Err(crate::ImError::unsupported(
                "sync-directory-hydrate-display-profiles",
            ))
        }
        #[cfg(not(feature = "sqlite"))]
        {
            let _ = request;
            Err(crate::ImError::unsupported(
                "directory-hydrate-display-profiles",
            ))
        }
    }

    pub async fn hydrate_display_profiles_async(
        &self,
        request: super::DisplayProfileBatchRequest,
    ) -> crate::ImResult<Vec<super::DisplayProfile>> {
        validate_display_profile_batch_request(&request)?;
        #[cfg(feature = "sqlite")]
        {
            let db = self.client.core_inner().local_state_db().await?;
            let mut result = Vec::with_capacity(request.peers.len());
            for peer in request.peers {
                let record = if peer.as_str().trim().starts_with("did:") {
                    db.get_contact_by_did(
                        self.owner_identity_id(),
                        self.owner_did().as_str(),
                        peer.as_str(),
                    )
                    .await
                } else {
                    db.get_current_contact_by_handle(
                        self.owner_identity_id(),
                        self.owner_did().as_str(),
                        peer.as_str(),
                    )
                    .await
                };
                result.push(display_profile_from_local_result(peer, record)?);
            }
            Ok(result)
        }
        #[cfg(not(feature = "sqlite"))]
        {
            let _ = request;
            Err(crate::ImError::unsupported(
                "directory-hydrate-display-profiles",
            ))
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
        #[cfg(all(feature = "sqlite", feature = "blocking"))]
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
        #[cfg(all(feature = "sqlite", not(feature = "blocking")))]
        {
            let _ = peer;
            Err(crate::ImError::unsupported(
                "sync-directory-relation-status",
            ))
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
        validate_not_self_peer(self.client, &request.peer)?;
        #[cfg(not(feature = "blocking"))]
        {
            let _ = request;
            Err(crate::ImError::unsupported("sync-directory-follow"))
        }
        #[cfg(feature = "blocking")]
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
        validate_not_self_peer(self.client, &request.peer)?;
        #[cfg(not(feature = "blocking"))]
        {
            let _ = request;
            Err(crate::ImError::unsupported("sync-directory-unfollow"))
        }
        #[cfg(feature = "blocking")]
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
        #[cfg(not(feature = "blocking"))]
        {
            let _ = peer;
            Err(crate::ImError::unsupported(
                "sync-directory-relationship-status",
            ))
        }
        #[cfg(feature = "blocking")]
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
        #[cfg(not(feature = "blocking"))]
        {
            let _ = query;
            Err(crate::ImError::unsupported("sync-directory-followers"))
        }
        #[cfg(feature = "blocking")]
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
        #[cfg(not(feature = "blocking"))]
        {
            let _ = query;
            Err(crate::ImError::unsupported("sync-directory-following"))
        }
        #[cfg(feature = "blocking")]
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

fn validate_display_profile_batch_request(
    request: &crate::directory::DisplayProfileBatchRequest,
) -> crate::ImResult<()> {
    if request
        .peers
        .iter()
        .any(|peer| peer.as_str().trim().is_empty())
    {
        return Err(crate::ImError::invalid_input(
            Some("peers".to_string()),
            "peers must not contain empty values",
        ));
    }
    Ok(())
}

#[cfg(all(feature = "sqlite", feature = "blocking"))]
fn local_contact_for_peer<FDid, FHandle>(
    by_did: FDid,
    by_handle: FHandle,
    peer: &crate::ids::PeerRef,
) -> crate::ImResult<crate::internal::contact_store::records::ContactRecord>
where
    FDid: FnOnce(&str) -> crate::ImResult<crate::internal::contact_store::records::ContactRecord>,
    FHandle:
        FnOnce(&str) -> crate::ImResult<crate::internal::contact_store::records::ContactRecord>,
{
    if peer.as_str().trim().starts_with("did:") {
        by_did(peer.as_str())
    } else {
        by_handle(peer.as_str())
    }
}

#[cfg(feature = "sqlite")]
fn display_profile_from_local_result(
    peer: crate::ids::PeerRef,
    record: crate::ImResult<crate::internal::contact_store::records::ContactRecord>,
) -> crate::ImResult<crate::directory::DisplayProfile> {
    match record {
        Ok(record) => crate::internal::contact_store::records::display_profile_from_record(&record),
        Err(crate::ImError::PeerNotFound { .. }) => display_profile_cache_miss(peer),
        Err(err) => Err(err),
    }
}

#[cfg(feature = "sqlite")]
fn display_profile_cache_miss(
    peer: crate::ids::PeerRef,
) -> crate::ImResult<crate::directory::DisplayProfile> {
    let mut warnings = vec!["display profile cache miss".to_string()];
    let (did, handle) = if peer.as_str().trim().starts_with("did:") {
        (Some(crate::ids::Did::parse(peer.as_str())?), None)
    } else {
        warnings.push("peer did is unknown until remote handle resolution".to_string());
        (None, Some(crate::ids::Handle::parse(peer.as_str(), "")?))
    };
    Ok(crate::directory::DisplayProfile {
        did,
        handle,
        display_name: None,
        avatar_uri: None,
        avatar_url: None,
        profile_uri: None,
        subject_type: None,
        cache_hit: false,
        warnings,
    })
}

fn validate_not_self_peer(
    client: &crate::core::ImClient,
    peer: &crate::ids::PeerRef,
) -> crate::ImResult<()> {
    if peer.as_str().trim() == client.did().as_str() {
        return Err(crate::ImError::invalid_input(
            Some("peer".to_string()),
            "cannot follow self",
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

#[cfg(feature = "sqlite")]
fn project_handle_lookup(
    client: &crate::core::ImClient,
    lookup: &super::HandleLookupResult,
) -> crate::ImResult<()> {
    let resolution = handle_lookup_resolution(lookup);
    let owner = crate::internal::local_state::owner_scope::OwnerScope::for_client(client)?;
    let mut connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    crate::internal::local_state::peer_personas::project_verified_handle(
        &mut connection,
        &owner.owner_identity_id,
        &owner.owner_did,
        lookup,
    )?;
    crate::internal::contact_store::projection::project_directory_resolution(client, &resolution);
    Ok(())
}

#[cfg(feature = "sqlite")]
async fn project_handle_lookup_async(
    client: &crate::core::ImClient,
    lookup: &super::HandleLookupResult,
) -> crate::ImResult<()> {
    let resolution = handle_lookup_resolution(lookup);
    let owner = crate::internal::local_state::owner_scope::OwnerScope::for_client(client)?;
    client
        .core_inner()
        .local_state_db()
        .await?
        .project_verified_handle(&owner.owner_identity_id, &owner.owner_did, lookup.clone())
        .await?;
    crate::internal::contact_store::projection::project_directory_resolution_async(
        client,
        &resolution,
    )
    .await
}

#[cfg(feature = "sqlite")]
fn handle_lookup_resolution(lookup: &super::HandleLookupResult) -> super::DirectoryResolution {
    super::DirectoryResolution {
        input: lookup.handle.as_str().to_owned(),
        did: lookup.did.clone(),
        handle: Some(lookup.handle.clone()),
        conversation_id: lookup.direct_conversation_id(),
        profile: lookup.profile.clone(),
        warnings: lookup.warnings.clone(),
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

#[cfg(all(test, feature = "sqlite"))]
mod direct_peer_route_projection_tests {
    use std::fs;

    fn lookup(current_did: &str) -> crate::directory::HandleLookupResult {
        crate::directory::HandleLookupResult {
            handle: crate::ids::Handle::parse("Bob.AWiki.Test", "").unwrap(),
            did: crate::ids::Did::parse(current_did).unwrap(),
            user_id: "user-bob".to_owned(),
            domain: Some("awiki.test".to_owned()),
            status: Some("active".to_owned()),
            binding_generation: Some("2".to_owned()),
            profile: None,
            warnings: Vec::new(),
        }
    }

    #[test]
    fn blocking_handle_projection_persists_route_before_any_message() {
        let fixture = Fixture::new("blocking");
        let client = fixture.client();
        let lookup = lookup("did:example:bob-old");

        super::project_handle_lookup(&client, &lookup).unwrap();

        let stored = fixture.load_route(&lookup.direct_conversation_id());
        assert_eq!(stored.current_did, "did:example:bob-old");
        assert_eq!(stored.peer_user_id, "user-bob");
        assert_eq!(stored.full_handle, "bob.awiki.test");
        assert_eq!(fixture.message_count(), 0);
    }

    #[tokio::test]
    async fn async_handle_projection_updates_route_after_did_rotation() {
        let fixture = Fixture::new("async-rotation");
        let client = fixture.client();
        let old = lookup("did:example:bob-old");
        super::project_handle_lookup_async(&client, &old)
            .await
            .unwrap();
        let current = lookup("did:example:bob-current");

        super::project_handle_lookup_async(&client, &current)
            .await
            .unwrap();

        let stored = fixture.load_route(&current.direct_conversation_id());
        assert_eq!(stored.current_did, "did:example:bob-current");
        assert_eq!(fixture.route_count(), 1);
        assert_eq!(fixture.message_count(), 0);
    }

    struct Fixture {
        root: tempfile::TempDir,
    }

    impl Fixture {
        fn new(prefix: &str) -> Self {
            let root = tempfile::Builder::new()
                .prefix(&format!("im-core-directory-route-{prefix}-"))
                .tempdir()
                .unwrap();
            let identities = root.path().join("identities");
            fs::create_dir_all(identities.join("alice")).unwrap();
            fs::write(identities.join("default"), "alice\n").unwrap();
            fs::write(
                identities.join("registry.json"),
                r#"{
                  "default_identity": "alice",
                  "identities": [{
                    "id": "alice-id",
                    "did": "did:example:alice",
                    "local_alias": "alice",
                    "ready_for_auth": true,
                    "ready_for_messaging": true,
                    "missing": []
                  }]
                }"#,
            )
            .unwrap();
            fs::write(identities.join("alice").join("did.json"), "{}").unwrap();
            Self { root }
        }

        fn client(&self) -> crate::core::ImClient {
            crate::core::ImCore::new(
                crate::ImCoreConfig {
                    service_base_url: crate::ServiceEndpoint::parse("https://example.test")
                        .unwrap(),
                    did_domain: "awiki.test".to_owned(),
                    user_service_endpoint: None,
                    message_service_endpoint: None,
                    mail_service_endpoint: None,
                    anp_service_endpoint: None,
                    anp_service_did: None,
                    ca_bundle: None,
                    transport_policy: crate::MessageTransportPolicy::HttpOnly,
                },
                crate::ImCorePaths {
                    identities: crate::IdentityRegistryPaths {
                        identity_root_dir: self.root.path().join("identities"),
                        registry_path: self.root.path().join("identities/registry.json"),
                        default_identity_path: Some(self.root.path().join("identities/default")),
                    },
                    local_state: crate::LocalStatePaths {
                        sqlite_path: self.sqlite_path(),
                    },
                    runtime: crate::RuntimePaths {
                        cache_dir: self.root.path().join("cache"),
                        temp_dir: self.root.path().join("tmp"),
                    },
                },
            )
            .unwrap()
            .client(crate::identity::IdentitySelector::LocalAlias(
                "alice".to_owned(),
            ))
            .unwrap()
        }

        fn sqlite_path(&self) -> std::path::PathBuf {
            self.root.path().join("local/im.sqlite")
        }

        fn load_route(
            &self,
            conversation_id: &str,
        ) -> crate::internal::local_state::direct_peer_routes::DirectPeerRouteRecord {
            let connection = crate::internal::local_state::open_writable(&self.sqlite_path())
                .expect("local state");
            crate::internal::local_state::direct_peer_routes::get(
                &connection,
                "alice-id",
                conversation_id,
            )
            .unwrap()
            .expect("route")
        }

        fn route_count(&self) -> i64 {
            self.count("direct_peer_routes")
        }

        fn message_count(&self) -> i64 {
            self.count("messages")
        }

        fn count(&self, table: &str) -> i64 {
            let connection = crate::internal::local_state::open_writable(&self.sqlite_path())
                .expect("local state");
            connection
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap()
        }
    }
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
