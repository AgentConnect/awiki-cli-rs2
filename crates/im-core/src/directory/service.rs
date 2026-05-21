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
        self.resolve_peer_with_runtime(peer, crate::internal::transport::UnavailableTransport)
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
        self.lookup_handle_with_runtime(handle, crate::internal::transport::UnavailableTransport)
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

    pub fn save_contact(
        &self,
        request: super::SaveContactRequest,
    ) -> crate::ImResult<super::Contact> {
        validate_save_contact(&request)?;
        let did = contact_did_from_request(&request)?;
        #[cfg(feature = "sqlite")]
        {
            let mut connection = crate::internal::contact_store::open_writable(self.client)?;
            let record = crate::internal::contact_store::projection::record_from_save_request(
                self.client,
                &request,
                did,
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
            return crate::internal::contact_store::records::contact_to_dto(&record);
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
            return Ok(crate::ids::Page {
                items,
                next_cursor: None,
                has_more: false,
            });
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
            return crate::internal::contact_store::records::relation_status_from_record(
                peer, record,
            );
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

    fn owner_identity_id(&self) -> &str {
        self.client.current_identity().id.as_str()
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

fn contact_did_from_request(
    request: &super::SaveContactRequest,
) -> crate::ImResult<crate::ids::Did> {
    if let Some(did) = &request.did {
        return Ok(did.clone());
    }
    if request.peer.as_str().starts_with("did:") {
        return crate::ids::Did::parse(request.peer.as_str());
    }
    Err(crate::ImError::invalid_input(
        Some("did".to_string()),
        "contact DID is required when peer is a handle",
    ))
}
