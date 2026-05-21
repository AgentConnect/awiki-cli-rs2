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
        crate::internal::directory_runtime::DirectoryRuntime::new(self.client, transport)
            .resolve_peer(peer)
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
        Err(crate::ImError::unsupported("directory-save-contact"))
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
        Err(crate::ImError::unsupported("directory-contacts"))
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
        Err(crate::ImError::unsupported("directory-relation-status"))
    }

    pub fn owner_did(&self) -> &crate::ids::Did {
        self.client.did()
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
