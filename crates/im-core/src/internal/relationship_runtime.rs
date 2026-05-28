use serde_json::Value;

use crate::internal::auth::session::{AsyncSessionProvider, SessionProvider};
use crate::internal::transport::{
    AsyncAuthenticatedRpcTransport, AsyncRpcTransport, AuthenticatedRpcTransport, RpcTransport,
};

pub(crate) struct RelationshipRuntime<'a, P, T> {
    client: &'a crate::core::ImClient,
    session_provider: P,
    transport: T,
}

impl<'a, P, T> RelationshipRuntime<'a, P, T> {
    pub(crate) fn new(
        client: &'a crate::core::ImClient,
        session_provider: P,
        transport: T,
    ) -> Self {
        Self {
            client,
            session_provider,
            transport,
        }
    }
}

impl<'a, P, T> RelationshipRuntime<'a, P, T>
where
    P: SessionProvider,
    T: AuthenticatedRpcTransport + RpcTransport,
{
    pub(crate) fn follow(
        mut self,
        request: crate::directory::FollowRequest,
    ) -> crate::ImResult<crate::directory::FollowResult> {
        let target = self.resolve_peer(request.peer.clone())?;
        reject_self_follow(self.client, &target.did)?;
        self.session_provider
            .ensure_session(crate::auth::AuthScope::UserProfile)?;
        let call = crate::internal::identity_wire::relationships::build_follow_rpc_call(
            target.did.as_str(),
        )?;
        let raw = self
            .transport
            .authenticated_rpc(call.endpoint, call.method, call.params)?;
        let mut warnings = Vec::new();
        let is_friend = bool_value(&raw, "is_friend");
        #[cfg(feature = "sqlite")]
        if let Err(err) = crate::internal::contact_store::relationships::record_follow_applied(
            self.client,
            &target.did,
            target.handle.as_ref(),
        ) {
            warnings.push(format!("Local relationship projection failed: {err}"));
        }
        let mut relation = match self.remote_status(request.peer.clone(), &target.did) {
            Ok(status) => status,
            Err(err) => {
                warnings.push(format!("Relationship status refresh failed: {err}"));
                local_relationship_status(self.client, request.peer.clone(), target.did.clone())?
            }
        };
        relation.is_following = true;
        relation.is_friend = relation.is_friend || is_friend;
        relation.warnings.extend(warnings.clone());
        Ok(crate::directory::FollowResult {
            peer: request.peer,
            did: target.did,
            is_friend,
            relation,
            warnings,
        })
    }

    pub(crate) fn unfollow(
        mut self,
        request: crate::directory::UnfollowRequest,
    ) -> crate::ImResult<crate::directory::UnfollowResult> {
        let target = self.resolve_peer(request.peer.clone())?;
        reject_self_follow(self.client, &target.did)?;
        self.session_provider
            .ensure_session(crate::auth::AuthScope::UserProfile)?;
        let call = crate::internal::identity_wire::relationships::build_unfollow_rpc_call(
            target.did.as_str(),
        )?;
        let raw = self
            .transport
            .authenticated_rpc(call.endpoint, call.method, call.params)?;
        let mut warnings = Vec::new();
        #[cfg(feature = "sqlite")]
        if let Err(err) = crate::internal::contact_store::relationships::record_unfollow_applied(
            self.client,
            &target.did,
            target.handle.as_ref(),
        ) {
            warnings.push(format!("Local relationship projection failed: {err}"));
        }
        let mut relation = match self.remote_status(request.peer.clone(), &target.did) {
            Ok(status) => status,
            Err(err) => {
                warnings.push(format!("Relationship status refresh failed: {err}"));
                let mut status = local_relationship_status(
                    self.client,
                    request.peer.clone(),
                    target.did.clone(),
                )?;
                status.is_following = false;
                status
            }
        };
        relation.is_following = false;
        relation.warnings.extend(warnings.clone());
        Ok(crate::directory::UnfollowResult {
            peer: request.peer,
            did: target.did,
            ok: bool_value(&raw, "ok") || raw.is_object(),
            relation,
            warnings,
        })
    }

    pub(crate) fn relationship_status(
        mut self,
        peer: crate::ids::PeerRef,
    ) -> crate::ImResult<crate::directory::RelationshipStatus> {
        let target = self.resolve_peer(peer.clone())?;
        self.session_provider
            .ensure_session(crate::auth::AuthScope::UserProfile)?;
        self.remote_status(peer, &target.did)
    }

    pub(crate) fn followers(
        mut self,
        query: crate::directory::RelationshipListQuery,
    ) -> crate::ImResult<crate::ids::Page<crate::directory::RelationshipListItem>> {
        self.relationship_list(query, RelationshipListKind::Followers)
    }

    pub(crate) fn following(
        mut self,
        query: crate::directory::RelationshipListQuery,
    ) -> crate::ImResult<crate::ids::Page<crate::directory::RelationshipListItem>> {
        self.relationship_list(query, RelationshipListKind::Following)
    }

    fn relationship_list(
        &mut self,
        query: crate::directory::RelationshipListQuery,
        kind: RelationshipListKind,
    ) -> crate::ImResult<crate::ids::Page<crate::directory::RelationshipListItem>> {
        let limit = query.limit.map(|limit| limit.0).unwrap_or(50);
        if limit == 0 {
            return Err(crate::ImError::invalid_input(
                Some("limit".to_string()),
                "limit must be greater than zero",
            ));
        }
        let offset = query.offset.unwrap_or(0);
        self.session_provider
            .ensure_session(crate::auth::AuthScope::UserProfile)?;
        let call = match kind {
            RelationshipListKind::Followers => {
                crate::internal::identity_wire::relationships::build_followers_rpc_call(
                    limit, offset,
                )?
            }
            RelationshipListKind::Following => {
                crate::internal::identity_wire::relationships::build_following_rpc_call(
                    limit, offset,
                )?
            }
        };
        let raw = self
            .transport
            .authenticated_rpc(call.endpoint, call.method, call.params)?;
        let items_raw = raw
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut items = Vec::new();
        for item in &items_raw {
            items.push(self.relationship_list_item(item, kind, query.hydrate_profiles)?);
        }
        Ok(crate::ids::Page {
            has_more: items.len() >= limit as usize,
            next_cursor: None,
            items,
        })
    }

    fn relationship_list_item(
        &mut self,
        raw: &Value,
        kind: RelationshipListKind,
        hydrate_profiles: bool,
    ) -> crate::ImResult<crate::directory::RelationshipListItem> {
        let did = match kind {
            RelationshipListKind::Followers => string_value(raw, "from_did"),
            RelationshipListKind::Following => string_value(raw, "to_did"),
        }
        .filter(|did| did.starts_with("did:"))
        .map(crate::ids::Did::parse)
        .transpose()?;
        let mut warnings = Vec::new();
        let mut profile = None;
        let mut handle = None;
        if hydrate_profiles {
            if let Some(did) = did.as_ref() {
                match self.public_profile_by_did(did) {
                    Ok(public) => {
                        handle = public.handle.clone();
                        profile = Some(public.profile);
                    }
                    Err(err) => warnings.push(format!("Public profile lookup failed: {err}")),
                }
            }
        }
        Ok(crate::directory::RelationshipListItem {
            did,
            handle,
            profile,
            created_at: string_value(raw, "created_at"),
            warnings,
        })
    }

    fn remote_status(
        &mut self,
        peer: crate::ids::PeerRef,
        did: &crate::ids::Did,
    ) -> crate::ImResult<crate::directory::RelationshipStatus> {
        let call =
            crate::internal::identity_wire::relationships::build_relationship_status_rpc_call(
                did.as_str(),
            )?;
        let raw = self
            .transport
            .authenticated_rpc(call.endpoint, call.method, call.params)?;
        let mut status = local_relationship_status(self.client, peer, did.clone())?;
        status.is_following = bool_value(&raw, "is_following");
        status.is_follower = bool_value(&raw, "is_follower");
        status.is_friend = bool_value(&raw, "is_friend");
        status.is_blocked = bool_value(&raw, "is_blocked");
        status.is_blocked_by = bool_value(&raw, "is_blocked_by");
        Ok(status)
    }

    fn resolve_peer(
        &mut self,
        peer: crate::ids::PeerRef,
    ) -> crate::ImResult<ResolvedRelationshipPeer> {
        if peer.as_str().trim().is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("peer".to_string()),
                "peer must not be empty",
            ));
        }
        if peer.as_str().trim().starts_with("did:") {
            return Ok(ResolvedRelationshipPeer {
                did: crate::ids::Did::parse(peer.as_str())?,
                handle: None,
            });
        }
        let result = crate::internal::directory_runtime::DirectoryRuntime::new(
            self.client,
            BorrowedRpcTransport(&mut self.transport),
        )
        .resolve_peer(peer)?;
        Ok(ResolvedRelationshipPeer {
            did: result.resolution.did,
            handle: result.resolution.handle,
        })
    }

    fn public_profile_by_did(
        &mut self,
        did: &crate::ids::Did,
    ) -> crate::ImResult<crate::directory::PublicProfile> {
        crate::internal::directory_runtime::DirectoryRuntime::new(
            self.client,
            BorrowedRpcTransport(&mut self.transport),
        )
        .public_profile(crate::directory::IdentitySubject::Did(did.clone()))
    }
}

impl<'a, P, T> RelationshipRuntime<'a, P, T>
where
    P: AsyncSessionProvider,
    T: AsyncAuthenticatedRpcTransport + AsyncRpcTransport,
{
    pub(crate) async fn follow_async(
        mut self,
        request: crate::directory::FollowRequest,
    ) -> crate::ImResult<crate::directory::FollowResult> {
        let target = self.resolve_peer_async(request.peer.clone()).await?;
        reject_self_follow(self.client, &target.did)?;
        self.session_provider
            .ensure_session(crate::auth::AuthScope::UserProfile)
            .await?;
        let call = crate::internal::identity_wire::relationships::build_follow_rpc_call(
            target.did.as_str(),
        )?;
        let raw = self
            .transport
            .authenticated_rpc(call.endpoint, call.method, call.params)
            .await?;
        let mut warnings = Vec::new();
        let is_friend = bool_value(&raw, "is_friend");
        #[cfg(feature = "sqlite")]
        if let Err(err) = record_relationship_projection_async(
            self.client,
            &target.did,
            target.handle.as_ref(),
            "following",
            true,
            "followed",
        )
        .await
        {
            warnings.push(format!("Local relationship projection failed: {err}"));
        }
        let mut relation = match self
            .remote_status_async(request.peer.clone(), &target.did)
            .await
        {
            Ok(status) => status,
            Err(err) => {
                warnings.push(format!("Relationship status refresh failed: {err}"));
                local_relationship_status_async(
                    self.client,
                    request.peer.clone(),
                    target.did.clone(),
                )
                .await?
            }
        };
        relation.is_following = true;
        relation.is_friend = relation.is_friend || is_friend;
        relation.warnings.extend(warnings.clone());
        Ok(crate::directory::FollowResult {
            peer: request.peer,
            did: target.did,
            is_friend,
            relation,
            warnings,
        })
    }

    pub(crate) async fn unfollow_async(
        mut self,
        request: crate::directory::UnfollowRequest,
    ) -> crate::ImResult<crate::directory::UnfollowResult> {
        let target = self.resolve_peer_async(request.peer.clone()).await?;
        reject_self_follow(self.client, &target.did)?;
        self.session_provider
            .ensure_session(crate::auth::AuthScope::UserProfile)
            .await?;
        let call = crate::internal::identity_wire::relationships::build_unfollow_rpc_call(
            target.did.as_str(),
        )?;
        let raw = self
            .transport
            .authenticated_rpc(call.endpoint, call.method, call.params)
            .await?;
        let mut warnings = Vec::new();
        #[cfg(feature = "sqlite")]
        if let Err(err) = record_relationship_projection_async(
            self.client,
            &target.did,
            target.handle.as_ref(),
            "none",
            false,
            "unfollowed",
        )
        .await
        {
            warnings.push(format!("Local relationship projection failed: {err}"));
        }
        let mut relation = match self
            .remote_status_async(request.peer.clone(), &target.did)
            .await
        {
            Ok(status) => status,
            Err(err) => {
                warnings.push(format!("Relationship status refresh failed: {err}"));
                let mut status = local_relationship_status_async(
                    self.client,
                    request.peer.clone(),
                    target.did.clone(),
                )
                .await?;
                status.is_following = false;
                status
            }
        };
        relation.is_following = false;
        relation.warnings.extend(warnings.clone());
        Ok(crate::directory::UnfollowResult {
            peer: request.peer,
            did: target.did,
            ok: bool_value(&raw, "ok") || raw.is_object(),
            relation,
            warnings,
        })
    }

    pub(crate) async fn relationship_status_async(
        mut self,
        peer: crate::ids::PeerRef,
    ) -> crate::ImResult<crate::directory::RelationshipStatus> {
        let target = self.resolve_peer_async(peer.clone()).await?;
        self.session_provider
            .ensure_session(crate::auth::AuthScope::UserProfile)
            .await?;
        self.remote_status_async(peer, &target.did).await
    }

    pub(crate) async fn followers_async(
        mut self,
        query: crate::directory::RelationshipListQuery,
    ) -> crate::ImResult<crate::ids::Page<crate::directory::RelationshipListItem>> {
        self.relationship_list_async(query, RelationshipListKind::Followers)
            .await
    }

    pub(crate) async fn following_async(
        mut self,
        query: crate::directory::RelationshipListQuery,
    ) -> crate::ImResult<crate::ids::Page<crate::directory::RelationshipListItem>> {
        self.relationship_list_async(query, RelationshipListKind::Following)
            .await
    }

    async fn relationship_list_async(
        &mut self,
        query: crate::directory::RelationshipListQuery,
        kind: RelationshipListKind,
    ) -> crate::ImResult<crate::ids::Page<crate::directory::RelationshipListItem>> {
        let limit = query.limit.map(|limit| limit.0).unwrap_or(50);
        if limit == 0 {
            return Err(crate::ImError::invalid_input(
                Some("limit".to_string()),
                "limit must be greater than zero",
            ));
        }
        let offset = query.offset.unwrap_or(0);
        self.session_provider
            .ensure_session(crate::auth::AuthScope::UserProfile)
            .await?;
        let call = match kind {
            RelationshipListKind::Followers => {
                crate::internal::identity_wire::relationships::build_followers_rpc_call(
                    limit, offset,
                )?
            }
            RelationshipListKind::Following => {
                crate::internal::identity_wire::relationships::build_following_rpc_call(
                    limit, offset,
                )?
            }
        };
        let raw = self
            .transport
            .authenticated_rpc(call.endpoint, call.method, call.params)
            .await?;
        let items_raw = raw
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut items = Vec::new();
        for item in &items_raw {
            items.push(
                self.relationship_list_item_async(item, kind, query.hydrate_profiles)
                    .await?,
            );
        }
        Ok(crate::ids::Page {
            has_more: items.len() >= limit as usize,
            next_cursor: None,
            items,
        })
    }

    async fn relationship_list_item_async(
        &mut self,
        raw: &Value,
        kind: RelationshipListKind,
        hydrate_profiles: bool,
    ) -> crate::ImResult<crate::directory::RelationshipListItem> {
        let did = match kind {
            RelationshipListKind::Followers => string_value(raw, "from_did"),
            RelationshipListKind::Following => string_value(raw, "to_did"),
        }
        .filter(|did| did.starts_with("did:"))
        .map(crate::ids::Did::parse)
        .transpose()?;
        let mut warnings = Vec::new();
        let mut profile = None;
        let mut handle = None;
        if hydrate_profiles {
            if let Some(did) = did.as_ref() {
                match self.public_profile_by_did_async(did).await {
                    Ok(public) => {
                        handle = public.handle.clone();
                        profile = Some(public.profile);
                    }
                    Err(err) => warnings.push(format!("Public profile lookup failed: {err}")),
                }
            }
        }
        Ok(crate::directory::RelationshipListItem {
            did,
            handle,
            profile,
            created_at: string_value(raw, "created_at"),
            warnings,
        })
    }

    async fn remote_status_async(
        &mut self,
        peer: crate::ids::PeerRef,
        did: &crate::ids::Did,
    ) -> crate::ImResult<crate::directory::RelationshipStatus> {
        let call =
            crate::internal::identity_wire::relationships::build_relationship_status_rpc_call(
                did.as_str(),
            )?;
        let raw = self
            .transport
            .authenticated_rpc(call.endpoint, call.method, call.params)
            .await?;
        let mut status = local_relationship_status_async(self.client, peer, did.clone()).await?;
        status.is_following = bool_value(&raw, "is_following");
        status.is_follower = bool_value(&raw, "is_follower");
        status.is_friend = bool_value(&raw, "is_friend");
        status.is_blocked = bool_value(&raw, "is_blocked");
        status.is_blocked_by = bool_value(&raw, "is_blocked_by");
        Ok(status)
    }

    async fn resolve_peer_async(
        &mut self,
        peer: crate::ids::PeerRef,
    ) -> crate::ImResult<ResolvedRelationshipPeer> {
        if peer.as_str().trim().is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("peer".to_string()),
                "peer must not be empty",
            ));
        }
        if peer.as_str().trim().starts_with("did:") {
            return Ok(ResolvedRelationshipPeer {
                did: crate::ids::Did::parse(peer.as_str())?,
                handle: None,
            });
        }
        let result = crate::internal::directory_runtime::DirectoryRuntime::new(
            self.client,
            BorrowedAsyncRpcTransport(&mut self.transport),
        )
        .resolve_peer_async(peer)
        .await?;
        Ok(ResolvedRelationshipPeer {
            did: result.resolution.did,
            handle: result.resolution.handle,
        })
    }

    async fn public_profile_by_did_async(
        &mut self,
        did: &crate::ids::Did,
    ) -> crate::ImResult<crate::directory::PublicProfile> {
        crate::internal::directory_runtime::DirectoryRuntime::new(
            self.client,
            BorrowedAsyncRpcTransport(&mut self.transport),
        )
        .public_profile_async(crate::directory::IdentitySubject::Did(did.clone()))
        .await
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedRelationshipPeer {
    did: crate::ids::Did,
    handle: Option<crate::ids::Handle>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RelationshipListKind {
    Followers,
    Following,
}

struct BorrowedRpcTransport<'a, T>(&'a mut T);

impl<T> RpcTransport for BorrowedRpcTransport<'_, T>
where
    T: RpcTransport,
{
    fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        self.0.rpc(endpoint, method, params)
    }
}

struct BorrowedAsyncRpcTransport<'a, T>(&'a mut T);

impl<T> AsyncRpcTransport for BorrowedAsyncRpcTransport<'_, T>
where
    T: AsyncRpcTransport,
{
    async fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        self.0.rpc(endpoint, method, params).await
    }
}

fn reject_self_follow(
    client: &crate::core::ImClient,
    did: &crate::ids::Did,
) -> crate::ImResult<()> {
    if did == client.did() {
        return Err(crate::ImError::invalid_input(
            Some("peer".to_string()),
            "cannot follow self",
        ));
    }
    Ok(())
}

fn local_relationship_status(
    client: &crate::core::ImClient,
    peer: crate::ids::PeerRef,
    did: crate::ids::Did,
) -> crate::ImResult<crate::directory::RelationshipStatus> {
    #[cfg(feature = "sqlite")]
    {
        let connection = match crate::internal::contact_store::open_writable(client) {
            Ok(connection) => connection,
            Err(err) => {
                let mut status = relationship_status_from_contact(peer, did, None)?;
                status
                    .warnings
                    .push(format!("Local relationship projection unavailable: {err}"));
                return Ok(status);
            }
        };
        let record = crate::internal::contact_store::records::get_contact_by_did(
            &connection,
            client.current_identity().id.as_str(),
            client.did().as_str(),
            did.as_str(),
        )
        .ok();
        relationship_status_from_contact(peer, did, record)
    }
    #[cfg(not(feature = "sqlite"))]
    {
        let _ = client;
        relationship_status_from_contact(peer, did, None)
    }
}

async fn local_relationship_status_async(
    client: &crate::core::ImClient,
    peer: crate::ids::PeerRef,
    did: crate::ids::Did,
) -> crate::ImResult<crate::directory::RelationshipStatus> {
    #[cfg(feature = "sqlite")]
    {
        let db = match client.core_inner().local_state_db().await {
            Ok(db) => db,
            Err(err) => {
                let mut status = relationship_status_from_contact(peer, did, None)?;
                status
                    .warnings
                    .push(format!("Local relationship projection unavailable: {err}"));
                return Ok(status);
            }
        };
        let record = db
            .get_contact_by_did(
                client.current_identity().id.as_str(),
                client.did().as_str(),
                did.as_str(),
            )
            .await
            .ok();
        relationship_status_from_contact(peer, did, record)
    }
    #[cfg(not(feature = "sqlite"))]
    {
        let _ = client;
        relationship_status_from_contact(peer, did, None)
    }
}

#[cfg(feature = "sqlite")]
async fn record_relationship_projection_async(
    client: &crate::core::ImClient,
    target_did: &crate::ids::Did,
    target_handle: Option<&crate::ids::Handle>,
    relationship: &str,
    followed: bool,
    event_type: &str,
) -> crate::ImResult<()> {
    let handle = target_handle
        .map(|handle| handle.as_str().to_string())
        .unwrap_or_default();
    let owner_identity_id = client.current_identity().id.as_str().to_string();
    let owner_did = client.did().as_str().to_string();
    let credential_name = client.current_identity().id.as_str().to_string();
    let db = client.core_inner().local_state_db().await?;
    db.upsert_contact(crate::internal::contact_store::records::ContactRecord {
        owner_identity_id: owner_identity_id.clone(),
        owner_did: owner_did.clone(),
        did: target_did.as_str().to_string(),
        handle: handle.clone(),
        relationship: relationship.to_string(),
        followed: Some(followed),
        source_type: "directory.relationship".to_string(),
        credential_name: credential_name.clone(),
        ..crate::internal::contact_store::records::ContactRecord::default()
    })
    .await?;
    db.append_relationship_event(
        crate::internal::contact_store::records::RelationshipEventRecord {
            owner_identity_id,
            owner_did,
            target_did: target_did.as_str().to_string(),
            target_handle: handle,
            event_type: event_type.to_string(),
            source_type: "directory.relationship".to_string(),
            status: "applied".to_string(),
            credential_name,
            ..crate::internal::contact_store::records::RelationshipEventRecord::default()
        },
    )
    .await?;
    Ok(())
}

#[cfg(feature = "sqlite")]
fn relationship_status_from_contact(
    peer: crate::ids::PeerRef,
    did: crate::ids::Did,
    record: Option<crate::internal::contact_store::records::ContactRecord>,
) -> crate::ImResult<crate::directory::RelationshipStatus> {
    Ok(crate::directory::RelationshipStatus {
        peer,
        did,
        is_following: false,
        is_follower: false,
        is_friend: false,
        is_blocked: false,
        is_blocked_by: false,
        is_contact: record.is_some(),
        messaged: record
            .as_ref()
            .and_then(|record| record.messaged)
            .unwrap_or(false),
        relationship: record
            .as_ref()
            .and_then(|record| optional_string(&record.relationship)),
        warnings: Vec::new(),
    })
}

#[cfg(not(feature = "sqlite"))]
fn relationship_status_from_contact(
    peer: crate::ids::PeerRef,
    did: crate::ids::Did,
    _record: Option<()>,
) -> crate::ImResult<crate::directory::RelationshipStatus> {
    Ok(crate::directory::RelationshipStatus {
        peer,
        did,
        is_following: false,
        is_follower: false,
        is_friend: false,
        is_blocked: false,
        is_blocked_by: false,
        is_contact: false,
        messaged: false,
        relationship: None,
        warnings: Vec::new(),
    })
}

fn bool_value(raw: &Value, key: &str) -> bool {
    raw.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn string_value(raw: &Value, key: &str) -> Option<String> {
    raw.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn optional_string(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}
