use std::collections::{HashMap, HashSet};

use serde_json::{json, Map, Value};

use crate::internal::auth::session::{AsyncSessionProvider, SessionProvider};
use crate::internal::transport::{
    AsyncAuthenticatedRpcTransport, AsyncRpcTransport, AuthenticatedRpcTransport, RpcTransport,
};
use crate::internal::wire::direct::DirectPayload;

pub(crate) const MESSAGE_RPC_ENDPOINT: &str = "/im/rpc";

pub(crate) struct MessageReadRuntime<'a, P, T, R> {
    client: &'a crate::core::ImClient,
    session_provider: P,
    transport: T,
    directory_transport: R,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InboxRead {
    pub query: crate::messages::InboxQuery,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HistoryRead {
    pub thread: crate::messages::ThreadRef,
    pub query: crate::messages::HistoryQuery,
    pub resolved_peer_did: Option<String>,
    pub peer_scope: Option<crate::internal::local_state::owner_scope::DirectPeerScope>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LocalHistoryRead {
    pub thread: crate::messages::ThreadRef,
    pub query: crate::messages::LocalHistoryQuery,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReadPageResult {
    pub page: crate::ids::Page<crate::messages::Message>,
    pub raw: Value,
}

impl<'a, P, T, R> MessageReadRuntime<'a, P, T, R>
where
    P: SessionProvider,
    T: AuthenticatedRpcTransport,
    R: RpcTransport,
{
    pub(crate) fn new(
        client: &'a crate::core::ImClient,
        session_provider: P,
        transport: T,
        directory_transport: R,
    ) -> Self {
        Self {
            client,
            session_provider,
            transport,
            directory_transport,
        }
    }

    pub(crate) fn inbox(mut self, input: InboxRead) -> crate::ImResult<ReadPageResult> {
        match input.query.scope {
            crate::messages::InboxScope::DirectOnly => self.direct_inbox(input.query),
            crate::messages::InboxScope::GroupOnly => self.group_inbox(input.query),
            crate::messages::InboxScope::All => self.all_inbox(input.query),
        }
    }

    fn all_inbox(&mut self, query: crate::messages::InboxQuery) -> crate::ImResult<ReadPageResult> {
        let requested_limit = query.limit;
        let mut direct_query = query.clone();
        direct_query.scope = crate::messages::InboxScope::DirectOnly;
        if query.inbox_history_options.is_some() {
            return self.direct_inbox(direct_query);
        }
        let mut direct = self.direct_inbox(direct_query)?;

        let mut group_query = query;
        group_query.scope = crate::messages::InboxScope::GroupOnly;
        let group = self.group_inbox(group_query)?;

        direct.page.items.extend(group.page.items);
        direct.page.has_more = direct.page.has_more || group.page.has_more;
        direct.page.has_more |=
            dedupe_and_truncate_messages(&mut direct.page.items, requested_limit);
        merge_raw_metadata(&mut direct.raw, &group.raw, "all");
        persist_projection_best_effort(self.client, &direct.page.items);
        Ok(direct)
    }

    fn direct_inbox(
        &mut self,
        query: crate::messages::InboxQuery,
    ) -> crate::ImResult<ReadPageResult> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::Messaging)?;
        let limit = page_limit(query.limit, 20);
        let delegated =
            delegated_inbox_context(self.client, query.inbox_history_options.as_ref(), limit)?;
        let service_did = delegated
            .as_ref()
            .map(|_| delegated_message_service_did(self.client));
        let mut params = crate::internal::wire::inbox::build_inbox_rpc_params(
            &crate::internal::wire::common::WireIdentity {
                did: self.client.did().as_str().to_string(),
            },
            crate::internal::wire::inbox::InboxWireRequest {
                limit,
                auth: delegated.as_ref().map(|context| {
                    crate::internal::wire::inbox::InboxWireAuth {
                        inbox_owner_did: context.inbox_owner_did.clone(),
                        inbox_auth_verification_method: context
                            .inbox_auth_verification_method
                            .clone(),
                        service_did: service_did.clone().unwrap_or_default(),
                    }
                }),
            },
        );
        if let Some(context) = delegated.as_ref() {
            attach_inbox_origin_proof(&mut params, "inbox.get", context)?;
        }
        let mut raw =
            self.transport
                .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "inbox.get", params)?;
        if delegated.is_some() {
            filter_delegated_e2ee_messages(&mut raw);
        } else {
            consume_group_e2ee_control_messages(self.client, &mut raw);
            project_secure_direct_messages(self.client, &mut raw, &mut self.directory_transport);
        }
        annotate_direct_peer_scopes(self.client, &mut raw, &mut self.directory_transport, None);
        let mut page = page_from_raw(self.client, &raw, query.limit)?;
        page.items.retain(|message| message.group.is_none());
        page.has_more |= dedupe_and_truncate_messages(&mut page.items, query.limit);
        persist_projection_best_effort(self.client, &page.items);
        Ok(ReadPageResult { page, raw })
    }

    fn group_inbox(
        &mut self,
        query: crate::messages::InboxQuery,
    ) -> crate::ImResult<ReadPageResult> {
        if query.inbox_history_options.is_some() {
            return Err(crate::ImError::unsupported("delegated-group-inbox"));
        }
        self.session_provider
            .ensure_session(crate::auth::AuthScope::GroupMessaging)?;
        let limit = page_limit(query.limit, 20);
        let group_list_params = crate::internal::wire::group::build_group_list_rpc_params(
            self.client.did().as_str(),
            limit,
        );
        let group_list_raw = self.transport.authenticated_rpc(
            MESSAGE_RPC_ENDPOINT,
            "group.list",
            group_list_params,
        )?;
        let groups = group_refs_from_group_list_raw(&group_list_raw);
        let mut items = Vec::new();
        let mut has_more = false;
        let mut raw = grouped_inbox_raw("group");
        merge_raw_metadata(&mut raw, &group_list_raw, "group");
        for group in groups {
            let params = crate::internal::wire::group::build_group_messages_rpc_params(
                self.client.did().as_str(),
                group.as_str(),
                limit,
                None,
                0,
            )?;
            let mut group_raw = self.transport.authenticated_rpc(
                MESSAGE_RPC_ENDPOINT,
                "group.list_messages",
                params,
            )?;
            project_group_e2ee_messages(self.client, &mut group_raw);
            let page =
                page_from_raw_with_group(self.client, &group_raw, query.limit, Some(&group))?;
            items.extend(page.items);
            has_more |= page.has_more;
            merge_raw_metadata(&mut raw, &group_raw, "group");
        }
        has_more |= dedupe_and_truncate_messages(&mut items, query.limit);
        let page = crate::ids::Page {
            items,
            next_cursor: None,
            has_more,
        };
        persist_projection_best_effort(self.client, &page.items);
        Ok(ReadPageResult { page, raw })
    }

    pub(crate) fn history(mut self, input: HistoryRead) -> crate::ImResult<ReadPageResult> {
        match input.thread {
            crate::messages::ThreadRef::Direct(peer) => {
                self.session_provider
                    .ensure_session(crate::auth::AuthScope::Messaging)?;
                let peer = direct_thread(peer, input.resolved_peer_did)?;
                let delegated = delegated_inbox_context(
                    self.client,
                    input.query.inbox_history_options.as_ref(),
                    page_limit(input.query.limit, 50),
                )?;
                let service_did = delegated
                    .as_ref()
                    .map(|_| delegated_message_service_did(self.client));
                let mut params = crate::internal::wire::history::build_history_rpc_params(
                    &crate::internal::wire::common::WireIdentity {
                        did: self.client.did().as_str().to_string(),
                    },
                    crate::internal::wire::history::HistoryWireRequest {
                        peer_did: peer.resolved_did.clone(),
                        limit: page_limit(input.query.limit, 50),
                        cursor: input.query.cursor.map(|cursor| cursor.as_str().to_string()),
                        skip: 0,
                        auth: delegated.as_ref().map(|context| {
                            crate::internal::wire::history::HistoryWireAuth {
                                inbox_owner_did: context.inbox_owner_did.clone(),
                                inbox_auth_verification_method: context
                                    .inbox_auth_verification_method
                                    .clone(),
                                service_did: service_did.clone().unwrap_or_default(),
                            }
                        }),
                    },
                )?;
                if let Some(context) = delegated.as_ref() {
                    attach_inbox_origin_proof(&mut params, "direct.get_history", context)?;
                }
                let mut raw = self.transport.authenticated_rpc(
                    MESSAGE_RPC_ENDPOINT,
                    "direct.get_history",
                    params,
                )?;
                if delegated.is_some() {
                    filter_delegated_e2ee_messages(&mut raw);
                } else {
                    consume_group_e2ee_control_messages(self.client, &mut raw);
                    project_secure_direct_messages(
                        self.client,
                        &mut raw,
                        &mut self.directory_transport,
                    );
                }
                annotate_direct_peer_scopes(
                    self.client,
                    &mut raw,
                    &mut self.directory_transport,
                    input.peer_scope.as_ref(),
                );
                let page = page_from_raw(self.client, &raw, input.query.limit)?;
                persist_projection_best_effort(self.client, &page.items);
                let page = merge_direct_local_projection_best_effort(
                    self.client,
                    page,
                    &peer.resolved_did,
                    input.peer_scope.as_ref(),
                    input.query.limit,
                );
                Ok(ReadPageResult { page, raw })
            }
            crate::messages::ThreadRef::Group(group) => {
                if input.query.inbox_history_options.is_some() {
                    return Err(crate::ImError::unsupported("delegated-group-history"));
                }
                self.session_provider
                    .ensure_session(crate::auth::AuthScope::GroupMessaging)?;
                let params = crate::internal::wire::group::build_group_messages_rpc_params(
                    self.client.did().as_str(),
                    group.as_str(),
                    page_limit(input.query.limit, 50),
                    input.query.cursor.as_ref().map(crate::ids::Cursor::as_str),
                    0,
                )?;
                let mut raw = self.transport.authenticated_rpc(
                    MESSAGE_RPC_ENDPOINT,
                    "group.list_messages",
                    params,
                )?;
                project_group_e2ee_messages(self.client, &mut raw);
                let mut page =
                    page_from_raw_with_group(self.client, &raw, input.query.limit, Some(&group))?;
                persist_projection_best_effort(self.client, &page.items);
                page = merge_group_local_projection_best_effort(
                    self.client,
                    page,
                    &group,
                    input.query.limit,
                );
                Ok(ReadPageResult { page, raw })
            }
            crate::messages::ThreadRef::Thread(_) => {
                Err(crate::ImError::unsupported("thread-history"))
            }
        }
    }

    pub(crate) fn local_history(self, input: LocalHistoryRead) -> crate::ImResult<ReadPageResult> {
        let page = local_history_page(self.client, input)?;
        Ok(ReadPageResult {
            page,
            raw: local_history_raw(),
        })
    }
}

impl<'a, P, T, R> MessageReadRuntime<'a, P, T, R>
where
    P: AsyncSessionProvider,
    T: AsyncAuthenticatedRpcTransport,
    R: AsyncRpcTransport,
{
    pub(crate) async fn inbox_async(mut self, input: InboxRead) -> crate::ImResult<ReadPageResult> {
        match input.query.scope {
            crate::messages::InboxScope::DirectOnly => self.direct_inbox_async(input.query).await,
            crate::messages::InboxScope::GroupOnly => self.group_inbox_async(input.query).await,
            crate::messages::InboxScope::All => self.all_inbox_async(input.query).await,
        }
    }

    async fn all_inbox_async(
        &mut self,
        query: crate::messages::InboxQuery,
    ) -> crate::ImResult<ReadPageResult> {
        let requested_limit = query.limit;
        let mut direct_query = query.clone();
        direct_query.scope = crate::messages::InboxScope::DirectOnly;
        if query.inbox_history_options.is_some() {
            return self.direct_inbox_async(direct_query).await;
        }
        let mut direct = self.direct_inbox_async(direct_query).await?;

        let mut group_query = query;
        group_query.scope = crate::messages::InboxScope::GroupOnly;
        let group = self.group_inbox_async(group_query).await?;

        direct.page.items.extend(group.page.items);
        direct.page.has_more = direct.page.has_more || group.page.has_more;
        direct.page.has_more |=
            dedupe_and_truncate_messages(&mut direct.page.items, requested_limit);
        merge_raw_metadata(&mut direct.raw, &group.raw, "all");
        persist_projection_best_effort_async(self.client, &direct.page.items).await;
        Ok(direct)
    }

    async fn direct_inbox_async(
        &mut self,
        query: crate::messages::InboxQuery,
    ) -> crate::ImResult<ReadPageResult> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::Messaging)
            .await?;
        let limit = page_limit(query.limit, 20);
        let delegated =
            delegated_inbox_context_async(self.client, query.inbox_history_options.as_ref(), limit)
                .await?;
        let service_did = delegated
            .as_ref()
            .map(|_| delegated_message_service_did(self.client));
        let mut params = crate::internal::wire::inbox::build_inbox_rpc_params(
            &crate::internal::wire::common::WireIdentity {
                did: self.client.did().as_str().to_string(),
            },
            crate::internal::wire::inbox::InboxWireRequest {
                limit,
                auth: delegated.as_ref().map(|context| {
                    crate::internal::wire::inbox::InboxWireAuth {
                        inbox_owner_did: context.inbox_owner_did.clone(),
                        inbox_auth_verification_method: context
                            .inbox_auth_verification_method
                            .clone(),
                        service_did: service_did.clone().unwrap_or_default(),
                    }
                }),
            },
        );
        if let Some(context) = delegated.as_ref() {
            attach_inbox_origin_proof(&mut params, "inbox.get", context)?;
        }
        let mut raw = self
            .transport
            .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "inbox.get", params)
            .await?;
        if delegated.is_some() {
            filter_delegated_e2ee_messages(&mut raw);
        } else {
            consume_group_e2ee_control_messages_async(self.client, &mut raw).await;
            project_secure_direct_messages_async(
                self.client,
                &mut raw,
                &mut self.directory_transport,
            )
            .await;
        }
        annotate_direct_peer_scopes_async(
            self.client,
            &mut raw,
            &mut self.directory_transport,
            None,
        )
        .await;
        let mut page = page_from_raw(self.client, &raw, query.limit)?;
        page.items.retain(|message| message.group.is_none());
        page.has_more |= dedupe_and_truncate_messages(&mut page.items, query.limit);
        persist_projection_best_effort_async(self.client, &page.items).await;
        Ok(ReadPageResult { page, raw })
    }

    async fn group_inbox_async(
        &mut self,
        query: crate::messages::InboxQuery,
    ) -> crate::ImResult<ReadPageResult> {
        if query.inbox_history_options.is_some() {
            return Err(crate::ImError::unsupported("delegated-group-inbox"));
        }
        self.session_provider
            .ensure_session(crate::auth::AuthScope::GroupMessaging)
            .await?;
        let limit = page_limit(query.limit, 20);
        let group_list_params = crate::internal::wire::group::build_group_list_rpc_params(
            self.client.did().as_str(),
            limit,
        );
        let group_list_raw = self
            .transport
            .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "group.list", group_list_params)
            .await?;
        let groups = group_refs_from_group_list_raw(&group_list_raw);
        let mut items = Vec::new();
        let mut has_more = false;
        let mut raw = grouped_inbox_raw("group");
        merge_raw_metadata(&mut raw, &group_list_raw, "group");
        for group in groups {
            let params = crate::internal::wire::group::build_group_messages_rpc_params(
                self.client.did().as_str(),
                group.as_str(),
                limit,
                None,
                0,
            )?;
            let mut group_raw = self
                .transport
                .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "group.list_messages", params)
                .await?;
            project_group_e2ee_messages_async(self.client, &mut group_raw).await;
            let page =
                page_from_raw_with_group(self.client, &group_raw, query.limit, Some(&group))?;
            items.extend(page.items);
            has_more |= page.has_more;
            merge_raw_metadata(&mut raw, &group_raw, "group");
        }
        has_more |= dedupe_and_truncate_messages(&mut items, query.limit);
        let page = crate::ids::Page {
            items,
            next_cursor: None,
            has_more,
        };
        persist_projection_best_effort_async(self.client, &page.items).await;
        Ok(ReadPageResult { page, raw })
    }

    pub(crate) async fn history_async(
        mut self,
        input: HistoryRead,
    ) -> crate::ImResult<ReadPageResult> {
        match input.thread {
            crate::messages::ThreadRef::Direct(peer) => {
                self.session_provider
                    .ensure_session(crate::auth::AuthScope::Messaging)
                    .await?;
                let peer = direct_thread(peer, input.resolved_peer_did)?;
                let delegated = delegated_inbox_context_async(
                    self.client,
                    input.query.inbox_history_options.as_ref(),
                    page_limit(input.query.limit, 50),
                )
                .await?;
                let service_did = delegated
                    .as_ref()
                    .map(|_| delegated_message_service_did(self.client));
                let mut params = crate::internal::wire::history::build_history_rpc_params(
                    &crate::internal::wire::common::WireIdentity {
                        did: self.client.did().as_str().to_string(),
                    },
                    crate::internal::wire::history::HistoryWireRequest {
                        peer_did: peer.resolved_did.clone(),
                        limit: page_limit(input.query.limit, 50),
                        cursor: input.query.cursor.map(|cursor| cursor.as_str().to_string()),
                        skip: 0,
                        auth: delegated.as_ref().map(|context| {
                            crate::internal::wire::history::HistoryWireAuth {
                                inbox_owner_did: context.inbox_owner_did.clone(),
                                inbox_auth_verification_method: context
                                    .inbox_auth_verification_method
                                    .clone(),
                                service_did: service_did.clone().unwrap_or_default(),
                            }
                        }),
                    },
                )?;
                if let Some(context) = delegated.as_ref() {
                    attach_inbox_origin_proof(&mut params, "direct.get_history", context)?;
                }
                let mut raw = self
                    .transport
                    .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "direct.get_history", params)
                    .await?;
                if delegated.is_some() {
                    filter_delegated_e2ee_messages(&mut raw);
                } else {
                    consume_group_e2ee_control_messages_async(self.client, &mut raw).await;
                    project_secure_direct_messages_async(
                        self.client,
                        &mut raw,
                        &mut self.directory_transport,
                    )
                    .await;
                }
                annotate_direct_peer_scopes_async(
                    self.client,
                    &mut raw,
                    &mut self.directory_transport,
                    input.peer_scope.as_ref(),
                )
                .await;
                let page = page_from_raw(self.client, &raw, input.query.limit)?;
                persist_projection_best_effort_async(self.client, &page.items).await;
                let page = merge_direct_local_projection_best_effort_async(
                    self.client,
                    page,
                    &peer.resolved_did,
                    input.peer_scope.as_ref(),
                    input.query.limit,
                )
                .await;
                Ok(ReadPageResult { page, raw })
            }
            crate::messages::ThreadRef::Group(group) => {
                if input.query.inbox_history_options.is_some() {
                    return Err(crate::ImError::unsupported("delegated-group-history"));
                }
                self.session_provider
                    .ensure_session(crate::auth::AuthScope::GroupMessaging)
                    .await?;
                let params = crate::internal::wire::group::build_group_messages_rpc_params(
                    self.client.did().as_str(),
                    group.as_str(),
                    page_limit(input.query.limit, 50),
                    input.query.cursor.as_ref().map(crate::ids::Cursor::as_str),
                    0,
                )?;
                let mut raw = self
                    .transport
                    .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "group.list_messages", params)
                    .await?;
                project_group_e2ee_messages_async(self.client, &mut raw).await;
                let mut page =
                    page_from_raw_with_group(self.client, &raw, input.query.limit, Some(&group))?;
                persist_projection_best_effort_async(self.client, &page.items).await;
                page = merge_group_local_projection_best_effort_async(
                    self.client,
                    page,
                    &group,
                    input.query.limit,
                )
                .await;
                Ok(ReadPageResult { page, raw })
            }
            crate::messages::ThreadRef::Thread(_) => {
                Err(crate::ImError::unsupported("thread-history"))
            }
        }
    }

    pub(crate) async fn local_history_async(
        self,
        input: LocalHistoryRead,
    ) -> crate::ImResult<ReadPageResult> {
        let page = local_history_page_async(self.client, input).await?;
        Ok(ReadPageResult {
            page,
            raw: local_history_raw(),
        })
    }
}

pub(crate) fn persist_projection_best_effort(
    client: &crate::core::ImClient,
    messages: &[crate::messages::Message],
) {
    if messages.is_empty() {
        return;
    }
    if matches!(
        crate::internal::message_runtime::local_projection::persist_remote_messages(
            client, messages,
        ),
        Ok(outcome) if outcome.stored_messages > 0
    ) {
        client.emit_committed_local_message_projection("remote_history");
    }
}

pub(crate) async fn persist_projection_best_effort_async(
    client: &crate::core::ImClient,
    messages: &[crate::messages::Message],
) {
    if messages.is_empty() {
        return;
    }
    if matches!(
        crate::internal::message_runtime::local_projection::persist_remote_messages_async(
            client, messages,
        )
        .await,
        Ok(outcome) if outcome.stored_messages > 0
    ) {
        client.emit_committed_local_message_projection("remote_history");
    }
}

fn delegated_message_service_did(client: &crate::core::ImClient) -> String {
    client
        .core_inner()
        .sdk_config()
        .anp_service_did
        .as_ref()
        .map(|did| did.as_str().to_owned())
        .unwrap_or_else(|| {
            format!(
                "did:wba:{}",
                client.core_inner().sdk_config().did_domain.trim()
            )
        })
}

pub(crate) struct DirectThread {
    pub(crate) resolved_did: String,
}

pub(crate) fn direct_thread(
    peer: crate::ids::PeerRef,
    resolved_peer_did: Option<String>,
) -> crate::ImResult<DirectThread> {
    let resolved = resolved_peer_did
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| peer.as_str().trim());
    if !resolved.starts_with("did:") {
        return Err(crate::ImError::PeerNotFound {
            peer: peer.as_str().to_string(),
        });
    }
    Ok(DirectThread {
        resolved_did: resolved.to_string(),
    })
}

pub(crate) fn page_limit(limit: crate::ids::PageLimit, fallback: i64) -> i64 {
    if limit.0 == 0 {
        fallback
    } else {
        i64::from(limit.0)
    }
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
fn merge_direct_local_projection_best_effort(
    client: &crate::core::ImClient,
    mut page: crate::ids::Page<crate::messages::Message>,
    peer_did: &str,
    peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
    requested_limit: crate::ids::PageLimit,
) -> crate::ids::Page<crate::messages::Message> {
    let Ok(connection) = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    ) else {
        return page;
    };
    let Ok(records) =
        crate::internal::local_state::messages::list_direct_messages_for_owner_identity(
            &connection,
            client.current_identity().id.as_str(),
            &direct_remote_history_conversation_ids(peer_did, peer_scope),
            page_limit(requested_limit, 50),
        )
    else {
        return page;
    };
    merge_local_message_records_into_page(&mut page, records, requested_limit);
    page
}

#[cfg(not(all(feature = "sqlite", any(feature = "blocking", test))))]
fn merge_direct_local_projection_best_effort(
    _client: &crate::core::ImClient,
    page: crate::ids::Page<crate::messages::Message>,
    _peer_did: &str,
    _peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
    _requested_limit: crate::ids::PageLimit,
) -> crate::ids::Page<crate::messages::Message> {
    page
}

#[cfg(feature = "sqlite")]
async fn merge_direct_local_projection_best_effort_async(
    client: &crate::core::ImClient,
    mut page: crate::ids::Page<crate::messages::Message>,
    peer_did: &str,
    peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
    requested_limit: crate::ids::PageLimit,
) -> crate::ids::Page<crate::messages::Message> {
    let db = match client.core_inner().local_state_db().await {
        Ok(db) => db,
        Err(_) => return page,
    };
    let records = match db
        .list_direct_messages(
            client.current_identity().id.as_str(),
            direct_remote_history_conversation_ids(peer_did, peer_scope),
            page_limit(requested_limit, 50),
        )
        .await
    {
        Ok(records) => records,
        Err(_) => {
            return page;
        }
    };
    if records.is_empty() {
        return page;
    }
    merge_local_message_records_into_page(&mut page, records, requested_limit);
    page
}

#[cfg(not(feature = "sqlite"))]
async fn merge_direct_local_projection_best_effort_async(
    _client: &crate::core::ImClient,
    page: crate::ids::Page<crate::messages::Message>,
    _peer_did: &str,
    _peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
    _requested_limit: crate::ids::PageLimit,
) -> crate::ids::Page<crate::messages::Message> {
    page
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
pub(crate) fn merge_group_local_projection_best_effort(
    client: &crate::core::ImClient,
    mut page: crate::ids::Page<crate::messages::Message>,
    group: &crate::ids::GroupRef,
    requested_limit: crate::ids::PageLimit,
) -> crate::ids::Page<crate::messages::Message> {
    let Ok(connection) = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    ) else {
        return page;
    };
    let Ok(rows) = crate::internal::local_state::groups::list_group_messages_for_owner_identity(
        &connection,
        client.current_identity().id.as_str(),
        client.did().as_str(),
        group.as_str(),
        page_limit(requested_limit, 50),
        None,
    ) else {
        return page;
    };
    merge_local_message_values_into_page(&mut page, rows, requested_limit);
    page
}

#[cfg(not(all(feature = "sqlite", any(feature = "blocking", test))))]
pub(crate) fn merge_group_local_projection_best_effort(
    _client: &crate::core::ImClient,
    page: crate::ids::Page<crate::messages::Message>,
    _group: &crate::ids::GroupRef,
    _requested_limit: crate::ids::PageLimit,
) -> crate::ids::Page<crate::messages::Message> {
    page
}

#[cfg(feature = "sqlite")]
pub(crate) async fn merge_group_local_projection_best_effort_async(
    client: &crate::core::ImClient,
    mut page: crate::ids::Page<crate::messages::Message>,
    group: &crate::ids::GroupRef,
    requested_limit: crate::ids::PageLimit,
) -> crate::ids::Page<crate::messages::Message> {
    let db = match client.core_inner().local_state_db().await {
        Ok(db) => db,
        Err(_) => return page,
    };
    let rows = match db
        .list_group_messages(
            client.current_identity().id.as_str(),
            client.did().as_str(),
            group.as_str(),
            page_limit(requested_limit, 50),
            None,
        )
        .await
    {
        Ok(rows) => rows,
        Err(_) => return page,
    };
    merge_local_message_values_into_page(&mut page, rows, requested_limit);
    page
}

#[cfg(not(feature = "sqlite"))]
pub(crate) async fn merge_group_local_projection_best_effort_async(
    _client: &crate::core::ImClient,
    page: crate::ids::Page<crate::messages::Message>,
    _group: &crate::ids::GroupRef,
    _requested_limit: crate::ids::PageLimit,
) -> crate::ids::Page<crate::messages::Message> {
    page
}

#[cfg(feature = "sqlite")]
fn merge_local_message_records_into_page(
    page: &mut crate::ids::Page<crate::messages::Message>,
    records: Vec<crate::internal::local_state::messages::MessageRecord>,
    requested_limit: crate::ids::PageLimit,
) {
    let local_messages = records
        .iter()
        .filter_map(|record| {
            crate::internal::message_runtime::conversations::message_from_record(record).ok()
        })
        .filter(|message| !is_direct_e2ee_wire_sdk_message(message))
        .collect::<Vec<_>>();
    merge_committed_projection_into_page(page, local_messages, requested_limit);
}

#[cfg(feature = "sqlite")]
fn merge_local_message_values_into_page(
    page: &mut crate::ids::Page<crate::messages::Message>,
    rows: Vec<serde_json::Value>,
    requested_limit: crate::ids::PageLimit,
) {
    let mut local_messages = rows
        .iter()
        .filter_map(|row| message_from_local_state_value(row).ok())
        .collect::<Vec<_>>();
    local_messages.retain(|local| {
        !page
            .items
            .iter()
            .any(|remote| remote.id != local.id && same_group_message_position(remote, local))
    });
    merge_committed_projection_into_page(page, local_messages, requested_limit);
}

#[cfg(feature = "sqlite")]
fn merge_committed_projection_into_page(
    page: &mut crate::ids::Page<crate::messages::Message>,
    local_messages: Vec<crate::messages::Message>,
    requested_limit: crate::ids::PageLimit,
) {
    if local_messages.is_empty() {
        return;
    }

    let mut committed_by_id = local_messages
        .into_iter()
        .map(|message| (message.id.as_str().to_owned(), message))
        .collect::<std::collections::HashMap<_, _>>();
    for message in &mut page.items {
        if let Some(committed) = committed_by_id.remove(message.id.as_str()) {
            *message = committed;
        }
    }
    page.items.extend(committed_by_id.into_values());
    page.has_more |= sort_dedupe_and_truncate_messages(&mut page.items, requested_limit);
}

#[cfg(feature = "sqlite")]
fn same_group_message_position(
    left: &crate::messages::Message,
    right: &crate::messages::Message,
) -> bool {
    let (Some(left_group), Some(right_group)) = (left.group.as_ref(), right.group.as_ref()) else {
        return false;
    };
    left_group == right_group
        && left.metadata.server_sequence.is_some()
        && left.metadata.server_sequence == right.metadata.server_sequence
}

#[cfg(feature = "sqlite")]
fn message_from_local_state_value(
    row: &serde_json::Value,
) -> crate::ImResult<crate::messages::Message> {
    let record = crate::internal::local_state::messages::MessageRecord {
        msg_id: string_value(row.get("msg_id")),
        owner_identity_id: string_value(row.get("owner_identity_id")),
        owner_did: string_value(row.get("owner_did")),
        conversation_id: string_value(row.get("conversation_id")),
        wire_thread_kind: string_value(row.get("wire_thread_kind")),
        wire_thread_ref: string_value(row.get("wire_thread_ref")),
        wire_identity_resolution_state: string_value(row.get("wire_identity_resolution_state")),
        thread_id: string_value(row.get("thread_id")),
        direction: row
            .get("direction")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or_default(),
        sender_did: string_value(row.get("sender_did")),
        receiver_did: string_value(row.get("receiver_did")),
        group_id: string_value(row.get("group_id")),
        group_did: string_value(row.get("group_did")),
        content_type: string_value(row.get("content_type")),
        content: string_value(row.get("content")),
        title: string_value(row.get("title")),
        server_seq: row.get("server_seq").and_then(serde_json::Value::as_i64),
        sent_at: string_value(row.get("sent_at")),
        stored_at: string_value(row.get("stored_at")),
        is_e2ee: bool_value(row.get("is_e2ee")).unwrap_or(false),
        is_read: bool_value(row.get("is_read")).unwrap_or(false),
        sender_name: string_value(row.get("sender_name")),
        metadata: string_value(row.get("metadata")),
        mentions_current_user: bool_value(row.get("mentions_current_user")).unwrap_or(false),
        credential_name: string_value(row.get("credential_name")),
    };
    crate::internal::message_runtime::conversations::message_from_record(&record)
}

#[cfg(feature = "sqlite")]
fn is_direct_e2ee_wire_sdk_message(message: &crate::messages::Message) -> bool {
    let content_type = message
        .metadata
        .content_type
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| match &message.body {
            crate::messages::MessageBodyView::Unsupported { content_type } => content_type
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or_default()
                .to_owned(),
            _ => String::new(),
        });
    anp::direct_e2ee::is_direct_e2ee_wire_content_type(&content_type)
}

#[cfg(feature = "sqlite")]
fn direct_local_history_conversation_ids(
    peer_did: &str,
    peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
) -> Vec<String> {
    let mut ids = Vec::new();
    if let Some(scope) = peer_scope {
        ids.push(
            crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(scope),
        );
    }
    let peer_did = peer_did.trim();
    if peer_did.starts_with("did:") {
        let did_id = crate::internal::local_state::owner_scope::direct_conversation_id(peer_did);
        if !ids.iter().any(|known| known == &did_id) {
            ids.push(did_id);
        }
    }
    ids
}

#[cfg(feature = "sqlite")]
fn direct_remote_history_conversation_ids(
    peer_did: &str,
    peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
) -> Vec<String> {
    if let Some(scope) = peer_scope {
        let mut ids = vec![
            crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(scope),
        ];
        let peer_did = peer_did.trim();
        if peer_did.starts_with("did:") && scope.user_id.trim() != peer_did {
            ids.push(crate::internal::local_state::owner_scope::direct_conversation_id(peer_did));
        }
        return ids;
    }
    direct_local_history_conversation_ids(peer_did, peer_scope)
}

pub(crate) fn sort_dedupe_and_truncate_messages(
    messages: &mut Vec<crate::messages::Message>,
    requested_limit: crate::ids::PageLimit,
) -> bool {
    messages.sort_by(|left, right| compare_messages_desc(left, right));
    dedupe_and_truncate_messages(messages, requested_limit)
}

fn compare_messages_desc(
    left: &crate::messages::Message,
    right: &crate::messages::Message,
) -> std::cmp::Ordering {
    match (
        left.metadata.server_sequence,
        right.metadata.server_sequence,
    ) {
        (Some(a), Some(b)) if a != b => b.cmp(&a),
        _ => message_timestamp(right)
            .cmp(&message_timestamp(left))
            .then_with(|| right.id.as_str().cmp(left.id.as_str())),
    }
}

fn message_timestamp(message: &crate::messages::Message) -> &str {
    message
        .sent_at
        .as_deref()
        .or(message.received_at.as_deref())
        .unwrap_or_default()
}

#[derive(Debug, Clone)]
struct DelegatedInboxContext {
    inbox_owner_did: String,
    inbox_auth_verification_method: String,
    did_document: Value,
    private_key_pem: String,
}

fn delegated_inbox_context(
    client: &crate::core::ImClient,
    options: Option<&crate::messages::InboxHistoryOptions>,
    _limit: i64,
) -> crate::ImResult<Option<DelegatedInboxContext>> {
    let Some(options) = options else {
        return Ok(None);
    };
    if options.inbox_auth.is_some() {
        return Err(crate::ImError::unsupported("scoped-inbox-token"));
    }
    let owner = required_inbox_option(options.inbox_owner_did.as_deref(), "inbox_owner_did")?;
    let method = required_inbox_option(
        options.inbox_auth_verification_method.as_deref(),
        "inbox_auth_verification_method",
    )?;
    let key_ref =
        required_inbox_option(options.inbox_auth_key_ref.as_deref(), "inbox_auth_key_ref")?;
    crate::internal::delegated_identity::require_method_owner(
        &owner,
        &method,
        "inbox_owner_did",
        "inbox_auth_verification_method",
    )?;
    let did_document =
        crate::internal::delegated_identity::load_did_document_for_owner(client, &owner, None)?;
    crate::internal::delegated_identity::require_authentication_method(
        &did_document,
        &method,
        "inbox_auth_verification_method",
    )?;
    let private_key_pem =
        crate::internal::delegated_identity::load_private_key_ref(client, &key_ref)?;
    Ok(Some(DelegatedInboxContext {
        inbox_owner_did: owner,
        inbox_auth_verification_method: method,
        did_document,
        private_key_pem,
    }))
}

async fn delegated_inbox_context_async(
    client: &crate::core::ImClient,
    options: Option<&crate::messages::InboxHistoryOptions>,
    _limit: i64,
) -> crate::ImResult<Option<DelegatedInboxContext>> {
    let Some(options) = options else {
        return Ok(None);
    };
    if options.inbox_auth.is_some() {
        return Err(crate::ImError::unsupported("scoped-inbox-token"));
    }
    let owner = required_inbox_option(options.inbox_owner_did.as_deref(), "inbox_owner_did")?;
    let method = required_inbox_option(
        options.inbox_auth_verification_method.as_deref(),
        "inbox_auth_verification_method",
    )?;
    let key_ref =
        required_inbox_option(options.inbox_auth_key_ref.as_deref(), "inbox_auth_key_ref")?;
    crate::internal::delegated_identity::require_method_owner(
        &owner,
        &method,
        "inbox_owner_did",
        "inbox_auth_verification_method",
    )?;
    let did_document = crate::internal::delegated_identity::load_did_document_for_owner_async(
        client, &owner, None,
    )
    .await?;
    crate::internal::delegated_identity::require_authentication_method(
        &did_document,
        &method,
        "inbox_auth_verification_method",
    )?;
    let private_key_pem =
        crate::internal::delegated_identity::load_private_key_ref_async(client, &key_ref).await?;
    Ok(Some(DelegatedInboxContext {
        inbox_owner_did: owner,
        inbox_auth_verification_method: method,
        did_document,
        private_key_pem,
    }))
}

fn required_inbox_option(value: Option<&str>, field: &str) -> crate::ImResult<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            crate::ImError::invalid_input(
                Some(field.to_owned()),
                format!("{field} is required when InboxHistoryOptions is set"),
            )
        })
}

fn attach_inbox_origin_proof(
    params: &mut Value,
    method: &str,
    context: &DelegatedInboxContext,
) -> crate::ImResult<()> {
    let payload = DirectPayload {
        method: method.to_owned(),
        meta: params
            .get("meta")
            .cloned()
            .ok_or_else(|| crate::ImError::Internal {
                message: "inbox/history params missing meta".to_owned(),
            })?,
        body: params
            .get("body")
            .cloned()
            .ok_or_else(|| crate::ImError::Internal {
                message: "inbox/history params missing body".to_owned(),
            })?,
    };
    let origin_proof = crate::internal::proof::origin::build_origin_proof(
        &crate::internal::proof::origin::OriginProofIdentity {
            identity_name: format!("delegated-inbox:{}", context.inbox_auth_verification_method),
            did_document: Some(context.did_document.clone()),
            key1_private_pem: context.private_key_pem.clone(),
            verification_method: Some(context.inbox_auth_verification_method.clone()),
        },
        &payload,
    )?;
    let Some(object) = params.as_object_mut() else {
        return Err(crate::ImError::Internal {
            message: "inbox/history params must be a JSON object".to_owned(),
        });
    };
    object.insert(
        "auth".to_owned(),
        crate::internal::proof::origin::origin_auth_value(&origin_proof),
    );
    Ok(())
}

pub(crate) fn page_from_raw(
    client: &crate::core::ImClient,
    raw: &Value,
    requested_limit: crate::ids::PageLimit,
) -> crate::ImResult<crate::ids::Page<crate::messages::Message>> {
    page_from_raw_with_group(client, raw, requested_limit, None)
}

pub(crate) fn page_from_raw_with_group(
    client: &crate::core::ImClient,
    raw: &Value,
    requested_limit: crate::ids::PageLimit,
    group: Option<&crate::ids::GroupRef>,
) -> crate::ImResult<crate::ids::Page<crate::messages::Message>> {
    let messages = raw
        .get("messages")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| message_from_value(client, item, group).transpose())
                .collect::<crate::ImResult<Vec<_>>>()
        })
        .transpose()?
        .unwrap_or_default();
    let limit = usize::try_from(requested_limit.0).unwrap_or_default();
    let has_more = raw
        .get("has_more")
        .and_then(Value::as_bool)
        .unwrap_or(limit > 0 && messages.len() >= limit);
    let next_cursor = raw
        .get("next_cursor")
        .or_else(|| raw.get("next_since_seq"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(crate::ids::Cursor::parse)
        .transpose()?;
    Ok(crate::ids::Page {
        items: messages,
        next_cursor,
        has_more,
    })
}

fn group_refs_from_group_list_raw(raw: &Value) -> Vec<crate::ids::GroupRef> {
    raw.get("groups")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(group_ref_from_group_value)
        .collect()
}

fn group_ref_from_group_value(value: &Value) -> Option<crate::ids::GroupRef> {
    let object = value.as_object()?;
    for key in ["group_did", "did", "id"] {
        if let Some(group) = object
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .and_then(|value| crate::ids::GroupRef::parse(value).ok())
        {
            return Some(group);
        }
    }
    None
}

fn grouped_inbox_raw(source: &str) -> Value {
    json!({
        "source": source,
        "warnings": [],
    })
}

fn local_history_raw() -> Value {
    json!({
        "source": "local",
        "warnings": [],
    })
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
fn local_history_page(
    client: &crate::core::ImClient,
    input: LocalHistoryRead,
) -> crate::ImResult<crate::ids::Page<crate::messages::Message>> {
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    let records =
        crate::internal::local_state::messages::list_messages_for_thread_ref_for_owner_identity(
            &connection,
            client.current_identity().id.as_str(),
            client.did().as_str(),
            &input.thread,
            page_limit(input.query.limit, 50),
            input.query.cursor.as_ref().map(crate::ids::Cursor::as_str),
        )?;
    local_history_records_to_page(records)
}

#[cfg(all(feature = "sqlite", not(any(feature = "blocking", test))))]
fn local_history_page(
    _client: &crate::core::ImClient,
    _input: LocalHistoryRead,
) -> crate::ImResult<crate::ids::Page<crate::messages::Message>> {
    Err(crate::ImError::unsupported("sync-message-local-history"))
}

#[cfg(not(feature = "sqlite"))]
fn local_history_page(
    _client: &crate::core::ImClient,
    _input: LocalHistoryRead,
) -> crate::ImResult<crate::ids::Page<crate::messages::Message>> {
    Err(crate::ImError::unsupported("message-local-history"))
}

#[cfg(feature = "sqlite")]
async fn local_history_page_async(
    client: &crate::core::ImClient,
    input: LocalHistoryRead,
) -> crate::ImResult<crate::ids::Page<crate::messages::Message>> {
    let records = client
        .core_inner()
        .local_state_db()
        .await?
        .list_messages_for_thread_ref(
            client.current_identity().id.as_str(),
            client.did().as_str(),
            input.thread,
            page_limit(input.query.limit, 50),
            input.query.cursor.map(|cursor| cursor.as_str().to_owned()),
        )
        .await?;
    local_history_records_to_page(records)
}

#[cfg(not(feature = "sqlite"))]
async fn local_history_page_async(
    _client: &crate::core::ImClient,
    _input: LocalHistoryRead,
) -> crate::ImResult<crate::ids::Page<crate::messages::Message>> {
    Err(crate::ImError::unsupported("message-local-history"))
}

#[cfg(feature = "sqlite")]
fn local_history_records_to_page(
    records: crate::internal::local_state::messages::ThreadLocalHistoryRecords,
) -> crate::ImResult<crate::ids::Page<crate::messages::Message>> {
    let next_cursor = records
        .next_cursor
        .map(crate::ids::Cursor::parse)
        .transpose()?;
    let items = records
        .records
        .iter()
        .map(crate::internal::message_runtime::conversations::message_from_record)
        .collect::<crate::ImResult<Vec<_>>>()?;
    Ok(crate::ids::Page {
        items,
        next_cursor,
        has_more: records.has_more,
    })
}

fn merge_raw_metadata(target: &mut Value, source: &Value, fallback_source: &str) {
    let Some(target_object) = target.as_object_mut() else {
        return;
    };
    let source_name = source
        .get("source")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback_source);
    target_object
        .entry("source".to_owned())
        .or_insert_with(|| Value::String(source_name.to_owned()));
    let warnings = source
        .get("warnings")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(|value| Value::String(value.to_owned()))
        .collect::<Vec<_>>();
    if warnings.is_empty() {
        return;
    }
    let entry = target_object
        .entry("warnings".to_owned())
        .or_insert_with(|| Value::Array(Vec::new()));
    if let Value::Array(items) = entry {
        items.extend(warnings);
    }
}

fn dedupe_and_truncate_messages(
    messages: &mut Vec<crate::messages::Message>,
    requested_limit: crate::ids::PageLimit,
) -> bool {
    let mut seen = HashSet::new();
    messages.retain(|message| seen.insert(message.id.as_str().to_owned()));
    let limit = usize::try_from(requested_limit.0).unwrap_or_default();
    if limit == 0 || messages.len() <= limit {
        return false;
    }
    messages.truncate(limit);
    true
}

pub(crate) fn annotate_direct_peer_scopes(
    client: &crate::core::ImClient,
    raw: &mut Value,
    directory_transport: &mut impl RpcTransport,
    preferred_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
) {
    let Some(messages) = raw.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };
    for message in messages {
        annotate_direct_peer_scope(client, message, directory_transport, preferred_scope);
    }
}

fn annotate_direct_peer_scope(
    client: &crate::core::ImClient,
    message: &mut Value,
    directory_transport: &mut impl RpcTransport,
    preferred_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
) {
    let Some(object) = message.as_object_mut() else {
        return;
    };
    if !string_value(object.get("group_did")).trim().is_empty() {
        return;
    }
    if !string_value(object.get("peer_user_id")).trim().is_empty()
        && !string_value(object.get("peer_full_handle"))
            .trim()
            .is_empty()
    {
        return;
    }
    let sender_did = string_value(object.get("sender_did"));
    let receiver_did = string_value(object.get("receiver_did"));
    let peer_did =
        direct_peer_did_for_message(client.did().as_str(), &sender_did, &receiver_did).trim();
    if peer_did.is_empty() || !peer_did.starts_with("did:") || peer_did == client.did().as_str() {
        return;
    }
    if let Some(scope) = preferred_scope {
        annotate_object_with_peer_scope(object, scope, Some(peer_did));
        return;
    }
    if let Some(scope) = resolve_direct_peer_scope(client, directory_transport, peer_did) {
        annotate_object_with_peer_scope(object, &scope, Some(peer_did));
    }
}

pub(crate) async fn annotate_direct_peer_scopes_async(
    client: &crate::core::ImClient,
    raw: &mut Value,
    directory_transport: &mut impl AsyncRpcTransport,
    preferred_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
) {
    let Some(messages) = raw.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };
    for message in messages {
        annotate_direct_peer_scope_async(client, message, directory_transport, preferred_scope)
            .await;
    }
}

async fn annotate_direct_peer_scope_async(
    client: &crate::core::ImClient,
    message: &mut Value,
    directory_transport: &mut impl AsyncRpcTransport,
    preferred_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
) {
    let Some(object) = message.as_object_mut() else {
        return;
    };
    if !string_value(object.get("group_did")).trim().is_empty() {
        return;
    }
    if !string_value(object.get("peer_user_id")).trim().is_empty()
        && !string_value(object.get("peer_full_handle"))
            .trim()
            .is_empty()
    {
        return;
    }
    let sender_did = string_value(object.get("sender_did"));
    let receiver_did = string_value(object.get("receiver_did"));
    let peer_did =
        direct_peer_did_for_message(client.did().as_str(), &sender_did, &receiver_did).trim();
    if peer_did.is_empty() || !peer_did.starts_with("did:") || peer_did == client.did().as_str() {
        return;
    }
    if let Some(scope) = preferred_scope {
        annotate_object_with_peer_scope(object, scope, Some(peer_did));
        return;
    }
    if let Some(scope) =
        resolve_direct_peer_scope_async(client, directory_transport, peer_did).await
    {
        annotate_object_with_peer_scope(object, &scope, Some(peer_did));
    }
}

enum VerifiedHandleScopeLookup {
    Verified(crate::internal::local_state::owner_scope::DirectPeerScope),
    Unavailable,
    Rejected,
}

fn resolve_direct_peer_scope(
    client: &crate::core::ImClient,
    directory_transport: &mut impl RpcTransport,
    peer_did: &str,
) -> Option<crate::internal::local_state::owner_scope::DirectPeerScope> {
    match lookup_direct_peer_scope(client, directory_transport, peer_did) {
        VerifiedHandleScopeLookup::Verified(scope) => Some(scope),
        VerifiedHandleScopeLookup::Rejected => None,
        VerifiedHandleScopeLookup::Unavailable => {
            let call =
                crate::internal::identity_wire::profile::build_profile_resolve_rpc_call(peer_did)
                    .ok()?;
            let raw = directory_transport
                .rpc(call.endpoint, call.method, call.params)
                .ok()?;
            direct_peer_scope_from_profile(raw)
        }
    }
}

fn lookup_direct_peer_scope(
    client: &crate::core::ImClient,
    directory_transport: &mut impl RpcTransport,
    peer_did: &str,
) -> VerifiedHandleScopeLookup {
    let Ok(call) =
        crate::internal::identity_wire::directory::build_handle_lookup_by_did_rpc_call(peer_did)
    else {
        return VerifiedHandleScopeLookup::Unavailable;
    };
    let Ok(raw) = directory_transport.rpc(call.endpoint, call.method, call.params) else {
        return VerifiedHandleScopeLookup::Unavailable;
    };
    let Ok(lookup) = crate::internal::directory_runtime::handle_lookup_from_value(&raw) else {
        return VerifiedHandleScopeLookup::Rejected;
    };
    if lookup.did.as_str() != peer_did {
        return VerifiedHandleScopeLookup::Rejected;
    }
    if crate::directory::project_handle_lookup(client, &lookup).is_err() {
        return VerifiedHandleScopeLookup::Rejected;
    }
    match crate::internal::local_state::owner_scope::DirectPeerScope::new(
        lookup.user_id,
        lookup.handle.as_str(),
    ) {
        Ok(scope) => VerifiedHandleScopeLookup::Verified(scope),
        Err(_) => VerifiedHandleScopeLookup::Rejected,
    }
}

async fn resolve_direct_peer_scope_async(
    client: &crate::core::ImClient,
    directory_transport: &mut impl AsyncRpcTransport,
    peer_did: &str,
) -> Option<crate::internal::local_state::owner_scope::DirectPeerScope> {
    match lookup_direct_peer_scope_async(client, directory_transport, peer_did).await {
        VerifiedHandleScopeLookup::Verified(scope) => Some(scope),
        VerifiedHandleScopeLookup::Rejected => None,
        VerifiedHandleScopeLookup::Unavailable => {
            let call =
                crate::internal::identity_wire::profile::build_profile_resolve_rpc_call(peer_did)
                    .ok()?;
            let raw = directory_transport
                .rpc(call.endpoint, call.method, call.params)
                .await
                .ok()?;
            direct_peer_scope_from_profile(raw)
        }
    }
}

async fn lookup_direct_peer_scope_async(
    client: &crate::core::ImClient,
    directory_transport: &mut impl AsyncRpcTransport,
    peer_did: &str,
) -> VerifiedHandleScopeLookup {
    let Ok(call) =
        crate::internal::identity_wire::directory::build_handle_lookup_by_did_rpc_call(peer_did)
    else {
        return VerifiedHandleScopeLookup::Unavailable;
    };
    let Ok(raw) = directory_transport
        .rpc(call.endpoint, call.method, call.params)
        .await
    else {
        return VerifiedHandleScopeLookup::Unavailable;
    };
    let Ok(lookup) = crate::internal::directory_runtime::handle_lookup_from_value(&raw) else {
        return VerifiedHandleScopeLookup::Rejected;
    };
    if lookup.did.as_str() != peer_did {
        return VerifiedHandleScopeLookup::Rejected;
    }
    if crate::directory::project_handle_lookup_async(client, &lookup)
        .await
        .is_err()
    {
        return VerifiedHandleScopeLookup::Rejected;
    }
    match crate::internal::local_state::owner_scope::DirectPeerScope::new(
        lookup.user_id,
        lookup.handle.as_str(),
    ) {
        Ok(scope) => VerifiedHandleScopeLookup::Verified(scope),
        Err(_) => VerifiedHandleScopeLookup::Rejected,
    }
}

fn direct_peer_scope_from_profile(
    raw: Value,
) -> Option<crate::internal::local_state::owner_scope::DirectPeerScope> {
    let user_id = first_string_at(
        &raw,
        &[
            "/user_id",
            "/userId",
            "/profile/user_id",
            "/profile/userId",
            "/result/user_id",
            "/result/userId",
        ],
    )?;
    let full_handle = first_string_at(
        &raw,
        &[
            "/full_handle",
            "/fullHandle",
            "/profile/full_handle",
            "/profile/fullHandle",
            "/result/full_handle",
            "/result/fullHandle",
        ],
    )
    .or_else(|| {
        let handle = first_string_at(
            &raw,
            &[
                "/handle",
                "/profile/handle",
                "/result/handle",
                "/local_name",
                "/result/local_name",
            ],
        )?;
        let domain = first_string_at(
            &raw,
            &[
                "/domain",
                "/profile/domain",
                "/result/domain",
                "/did_domain",
                "/result/did_domain",
            ],
        )?;
        Some(format!(
            "{}.{}",
            handle.trim().trim_start_matches('@'),
            domain.trim()
        ))
    })?;
    crate::internal::local_state::owner_scope::DirectPeerScope::new(user_id, full_handle).ok()
}

fn first_string_at(raw: &Value, pointers: &[&str]) -> Option<String> {
    for pointer in pointers {
        let value = raw
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let Some(value) = value {
            return Some(value.to_owned());
        }
    }
    None
}

fn annotate_object_with_peer_scope(
    object: &mut Map<String, Value>,
    scope: &crate::internal::local_state::owner_scope::DirectPeerScope,
    current_did: Option<&str>,
) {
    object.insert(
        "peer_user_id".to_owned(),
        Value::String(scope.user_id.to_owned()),
    );
    object.insert(
        "peer_full_handle".to_owned(),
        Value::String(scope.full_handle.to_owned()),
    );
    if let Some(did) = current_did.map(str::trim).filter(|value| !value.is_empty()) {
        object.insert("peer_current_did".to_owned(), Value::String(did.to_owned()));
        object.insert(
            "resolved_target_did".to_owned(),
            Value::String(did.to_owned()),
        );
    }
}

pub(crate) fn project_secure_direct_messages(
    client: &crate::core::ImClient,
    raw: &mut Value,
    directory_transport: &mut impl RpcTransport,
) {
    project_secure_direct_messages_impl(client, raw, directory_transport, true);
}

fn filter_delegated_e2ee_messages(raw: &mut Value) {
    let Some(messages) = raw.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };
    messages.retain(|message| !is_delegated_e2ee_message(message));
}

fn is_delegated_e2ee_message(message: &Value) -> bool {
    if is_p5_v2_projection_candidate(message) || is_p6_v2_projection_candidate(message) {
        return true;
    }
    let content_type = direct_message_content_type(message);
    if anp::direct_e2ee::is_direct_e2ee_wire_content_type(&content_type) {
        return true;
    }
    #[cfg(feature = "group-e2ee")]
    {
        if content_type == crate::internal::group_e2ee::wire::GROUP_E2EE_CIPHER_CONTENT_TYPE {
            return true;
        }
    }
    let security = message
        .get("message_security_profile")
        .or_else(|| message.get("security_profile"))
        .or_else(|| message.get("security"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    matches!(security, "direct-e2ee" | "group-e2ee")
}

fn project_secure_direct_messages_impl(
    client: &crate::core::ImClient,
    raw: &mut Value,
    directory_transport: &mut impl RpcTransport,
    redact_attachment_secrets: bool,
) {
    #[cfg(not(feature = "sqlite"))]
    {
        let _ = (client, raw, directory_transport, redact_attachment_secrets);
    }
    #[cfg(feature = "sqlite")]
    {
        let Some(messages) = raw.get_mut("messages").and_then(Value::as_array_mut) else {
            return;
        };
        let mut message_values = std::mem::take(messages);
        // Root-control delivery and the P5 v2 session handshake are async-only
        // because they require authenticated network calls. A synchronous read
        // must still fail closed by removing both control classes from every
        // ordinary Inbox/History projection, independently of the rollout gate.
        message_values.retain(|message| {
            message.get("private_transport_context").is_none()
                && !is_v2_session_control_projection(message)
                && !is_p5_v2_projection_candidate(message)
        });
        let _ = apply_cached_secure_direct_messages(client, &mut message_values);
        let warnings =
            crate::internal::secure_direct::incoming::maybe_decrypt_direct_e2ee_messages_for_client(
                client,
                &mut message_values,
                directory_transport,
                crate::internal::secure_direct::incoming::DirectDecryptMode::ReadOnly,
            );
        let filtered =
            crate::internal::secure_direct::incoming::filter_displayable_direct_e2ee_messages(
                message_values,
            );
        let mut filtered = filtered;
        cache_attachment_manifests_for_internal_download(client, &filtered);
        if redact_attachment_secrets {
            redact_attachment_manifests_for_public_projection(&mut filtered);
        }
        *messages = filtered;
        append_secure_direct_warnings(raw, warnings);
    }
}

#[cfg(feature = "sqlite")]
pub(crate) fn project_secure_direct_messages_for_attachment_download(
    client: &crate::core::ImClient,
    raw: &mut Value,
    directory_transport: &mut impl RpcTransport,
) {
    project_secure_direct_messages_impl(client, raw, directory_transport, false);
}

pub(crate) async fn project_secure_direct_messages_async(
    client: &crate::core::ImClient,
    raw: &mut Value,
    directory_transport: &mut impl AsyncRpcTransport,
) {
    project_secure_direct_messages_async_impl(client, raw, directory_transport, true).await;
}

async fn project_secure_direct_messages_async_impl(
    client: &crate::core::ImClient,
    raw: &mut Value,
    directory_transport: &mut impl AsyncRpcTransport,
    redact_attachment_secrets: bool,
) {
    #[cfg(not(feature = "sqlite"))]
    {
        let _ = (client, raw, directory_transport, redact_attachment_secrets);
    }
    #[cfg(feature = "sqlite")]
    {
        let Some(messages) = raw.get_mut("messages").and_then(Value::as_array_mut) else {
            return;
        };
        let mut message_values = std::mem::take(messages);
        let cached_secure_indices =
            apply_cached_secure_direct_messages_async(client, &mut message_values).await;
        let mut processed_async = vec![false; message_values.len()];
        let mut async_warnings = Vec::new();
        let mut pending_cipher_indices: HashMap<String, Vec<usize>> = HashMap::new();
        let async_receive =
            crate::internal::secure_direct::async_receive::AsyncDirectSecureIncomingProcessor::new(
                client,
            );
        let mut order = (0..message_values.len()).collect::<Vec<_>>();
        order.sort_by(|left, right| {
            compare_secure_direct_message_order(&message_values[*left], &message_values[*right])
        });
        for index in order {
            if message_values[index]
                .get("private_transport_context")
                .is_some()
            {
                // Only the same-domain inbox projection can set this marker.
                // It must never enter normal Direct rendering or the v1
                // fallback, including while rollout is disabled or malformed.
                mark_private_root_control(&mut message_values[index]);
                processed_async[index] = true;
                if !client.core_inner().root_key_transfer_enabled() {
                    continue;
                }
                let (metadata, body, transport_context) =
                    match parse_private_root_control(&message_values[index]) {
                        Ok(value) => value,
                        Err(_) => continue,
                    };
                let core = client.core_handle();
                let _ = crate::internal::identity_root_transfer_runtime::receive_root_control(
                    &core,
                    client,
                    metadata,
                    body,
                    transport_context,
                )
                .await;
                continue;
            }
            // Recognition is gate-independent: disabling rollout suppresses
            // side effects, never the confidentiality filter. A recognized or
            // malformed control candidate cannot fall through to ordinary
            // Direct decryption/rendering.
            match parse_v2_session_control(&message_values[index]) {
                Ok(Some((metadata, body))) => {
                    mark_private_root_control(&mut message_values[index]);
                    processed_async[index] = true;
                    if client.core_inner().root_key_transfer_enabled() {
                        let core = client.core_handle();
                        let _ = crate::internal::identity_root_transfer_runtime::receive_session_control(
                            &core,
                            client,
                            metadata,
                            body,
                        )
                        .await;
                    }
                    continue;
                }
                Ok(None) => {}
                Err(_) => {
                    mark_private_root_control(&mut message_values[index]);
                    processed_async[index] = true;
                    continue;
                }
            }
            if is_p5_v2_projection_candidate(&message_values[index]) {
                processed_async[index] = true;
                if cached_secure_indices.contains(&index) {
                    continue;
                }
                if !client.core_inner().direct_e2ee_v2_enabled() {
                    mark_private_root_control(&mut message_values[index]);
                    continue;
                }
                let wire_message = p5_v2_wire_projection(&message_values[index]);
                let (metadata, body) =
                    match crate::internal::secure_direct::v2_product::parse_v2_wire_message(
                        &wire_message,
                    ) {
                        Ok(Some(value)) => value,
                        Ok(None) | Err(_) => {
                            mark_private_root_control(&mut message_values[index]);
                            continue;
                        }
                    };
                let core = client.core_handle();
                match crate::internal::secure_direct::v2_product::receive_for_client(
                    &core, client, true, metadata, body,
                )
                .await
                {
                    Ok(outcome) => apply_p5_v2_product_outcome(&mut message_values[index], outcome),
                    Err(_) => mark_private_root_control(&mut message_values[index]),
                }
                continue;
            }
            let content_type = direct_message_content_type(&message_values[index]);
            let notification = match crate::internal::secure_direct::incoming::direct_e2ee_notification_from_message_view(&message_values[index]) {
                Ok(notification) => notification,
                Err(_) => continue,
            };
            let pending_cipher_message_id = if content_type == "application/anp-direct-cipher+json"
            {
                direct_notification_message_id(&notification)
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| secure_direct_message_id(&message_values[index]))
            } else {
                String::new()
            };
            let result = if content_type == "application/anp-direct-init+json" {
                let sender_did = notification
                    .get("meta")
                    .and_then(Value::as_object)
                    .and_then(|meta| meta.get("sender_did"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let sender_document = match resolve_direct_sender_document_async(
                    client,
                    directory_transport,
                    &sender_did,
                )
                .await
                {
                    Ok(document) => document,
                    Err(err) => {
                        mark_async_direct_failure(
                            &mut message_values[index],
                            &mut async_warnings,
                            err,
                        );
                        continue;
                    }
                };
                async_receive
                    .process_init_if_ready(notification, sender_document)
                    .await
            } else if content_type == "application/anp-direct-cipher+json" {
                async_receive.process_cipher_if_ready(notification).await
            } else {
                continue;
            };
            match result {
                Ok(crate::internal::secure_direct::async_receive::AsyncDirectSecureReceiveOutcome::Processed(result)) => {
                    crate::internal::secure_direct::incoming::apply_direct_e2ee_processing_result(
                        &mut message_values[index],
                        &Value::Object(result),
                    );
                    processed_async[index] = true;
                }
                Ok(crate::internal::secure_direct::async_receive::AsyncDirectSecureReceiveOutcome::ProcessedWithReplay {
                    result,
                    replayed,
                }) => {
                    crate::internal::secure_direct::incoming::apply_direct_e2ee_processing_result(
                        &mut message_values[index],
                        &Value::Object(result),
                    );
                    processed_async[index] = true;
                    for replay in replayed {
                        let Some(indices) = pending_cipher_indices.get_mut(&replay.message_id) else {
                            continue;
                        };
                        let Some(pending_index) = indices.pop() else {
                            continue;
                        };
                        crate::internal::secure_direct::incoming::apply_direct_e2ee_processing_result(
                            &mut message_values[pending_index],
                            &Value::Object(replay.result),
                        );
                        processed_async[pending_index] = true;
                    }
                }
                Ok(crate::internal::secure_direct::async_receive::AsyncDirectSecureReceiveOutcome::Fallback(
                    crate::internal::secure_direct::async_receive::AsyncDirectSecureReceiveFallback::NoEstablishedSession,
                )) if content_type == "application/anp-direct-cipher+json" => {
                    if !pending_cipher_message_id.trim().is_empty() {
                        pending_cipher_indices
                            .entry(pending_cipher_message_id)
                            .or_default()
                            .push(index);
                    }
                    continue;
                }
                Ok(crate::internal::secure_direct::async_receive::AsyncDirectSecureReceiveOutcome::Fallback(_)) => {
                    continue;
                }
                Err(err) => {
                    mark_async_direct_failure(&mut message_values[index], &mut async_warnings, err);
                    continue;
                }
            };
        }
        #[cfg(feature = "blocking")]
        {
            let mut fallback_entries = message_values
                .iter()
                .cloned()
                .enumerate()
                .filter(|(index, message)| {
                    !processed_async[*index] && is_direct_e2ee_wire_message(message)
                })
                .collect::<Vec<_>>();
            let mut fallback_messages = fallback_entries
                .iter()
                .map(|(_, message)| message.clone())
                .collect::<Vec<_>>();
            let warnings = if fallback_messages.is_empty() {
                Vec::new()
            } else {
                crate::internal::secure_direct::incoming::maybe_decrypt_direct_e2ee_messages_for_client(
                    client,
                    &mut fallback_messages,
                    &mut crate::internal::transport::CoreHttpTransport::new(client),
                    crate::internal::secure_direct::incoming::DirectDecryptMode::ReadOnly,
                )
            };
            async_warnings.extend(warnings);
            for ((index, _), message) in fallback_entries.drain(..).zip(fallback_messages) {
                message_values[index] = message;
            }
        }
        #[cfg(not(feature = "blocking"))]
        {
            let _ = client;
            for (index, message) in message_values.iter_mut().enumerate() {
                if !processed_async[index] && is_direct_e2ee_wire_message(message) {
                    mark_async_direct_failure(
                        message,
                        &mut async_warnings,
                        crate::ImError::unsupported("sync-direct-e2ee-read-fallback"),
                    );
                }
            }
        }
        let filtered =
            crate::internal::secure_direct::incoming::filter_displayable_direct_e2ee_messages(
                message_values,
            );
        let mut filtered = filtered;
        cache_attachment_manifests_for_internal_download_async(client, &filtered).await;
        if redact_attachment_secrets {
            redact_attachment_manifests_for_public_projection(&mut filtered);
        }
        *messages = filtered;
        append_secure_direct_warnings(raw, compact_secure_direct_warnings(async_warnings));
    }
}

#[cfg(feature = "sqlite")]
pub(crate) async fn project_secure_direct_messages_for_attachment_download_async(
    client: &crate::core::ImClient,
    raw: &mut Value,
    directory_transport: &mut impl AsyncRpcTransport,
) {
    project_secure_direct_messages_async_impl(client, raw, directory_transport, false).await;
}

async fn resolve_direct_sender_document_async(
    client: &crate::core::ImClient,
    directory_transport: &mut impl AsyncRpcTransport,
    did: &str,
) -> crate::ImResult<Value> {
    if did == client.did().as_str() {
        return client.runtime().key_provider.did_document();
    }
    let call = crate::internal::identity_wire::profile::build_profile_resolve_rpc_call(did)?;
    match directory_transport
        .rpc(call.endpoint, call.method, call.params)
        .await
        .and_then(|raw| {
            did_document_from_resolve(raw).ok_or_else(|| crate::ImError::PeerNotFound {
                peer: did.to_owned(),
            })
        }) {
        Ok(document) => Ok(document),
        Err(err) => match crate::internal::identity_document_cache::load_local_did_document_async(
            &client.core_inner().sdk_paths().identities,
            did,
        )
        .await
        {
            Ok(Some(document)) => Ok(document),
            Ok(None) | Err(_) => Err(err),
        },
    }
}

async fn read_json_file_async(path: std::path::PathBuf, path_kind: &str) -> crate::ImResult<Value> {
    let raw =
        tokio::fs::read(&path)
            .await
            .map_err(|err| crate::ImError::CredentialFileUnreadable {
                path_kind: path_kind.to_owned(),
                detail: err.to_string(),
            })?;
    serde_json::from_slice(&raw).map_err(|err| crate::ImError::Serialization {
        detail: err.to_string(),
    })
}

fn did_document_from_resolve(value: Value) -> Option<Value> {
    if looks_like_did_document(&value) {
        return Some(value);
    }
    for pointer in [
        "/did_document",
        "/didDocument",
        "/document",
        "/profile/did_document",
        "/profile/didDocument",
        "/result/did_document",
        "/result/didDocument",
    ] {
        let candidate = value.pointer(pointer)?;
        if looks_like_did_document(candidate) {
            return Some(candidate.clone());
        }
    }
    None
}

fn looks_like_did_document(value: &Value) -> bool {
    value
        .get("id")
        .and_then(Value::as_str)
        .is_some_and(|value| value.starts_with("did:"))
        && value.get("verificationMethod").is_some()
}

#[cfg(feature = "sqlite")]
fn parse_private_root_control(
    message: &Value,
) -> crate::ImResult<(
    anp::direct_e2ee::V2DirectMetadata,
    anp::direct_e2ee::V2DirectCipherBody,
    crate::internal::identity_root_transfer::RootImportTransportContext,
)> {
    let object = message
        .as_object()
        .ok_or(crate::ImError::PermissionDenied)?;
    let metadata = serde_json::from_value(
        object
            .get("meta")
            .cloned()
            .ok_or(crate::ImError::PermissionDenied)?,
    )
    .map_err(|_| crate::ImError::PermissionDenied)?;
    let body = serde_json::from_value(
        object
            .get("body")
            .cloned()
            .ok_or(crate::ImError::PermissionDenied)?,
    )
    .map_err(|_| crate::ImError::PermissionDenied)?;
    let transport_context = serde_json::from_value(
        object
            .get("private_transport_context")
            .cloned()
            .ok_or(crate::ImError::PermissionDenied)?,
    )
    .map_err(|_| crate::ImError::PermissionDenied)?;
    Ok((metadata, body, transport_context))
}

#[cfg(feature = "sqlite")]
pub(crate) fn parse_v2_session_control(
    message: &Value,
) -> crate::ImResult<
    Option<(
        anp::direct_e2ee::V2DirectMetadata,
        anp::direct_e2ee::V2DirectBody,
    )>,
> {
    let Some(object) = message.as_object() else {
        return Ok(None);
    };
    let Some(meta_value) = object.get("meta") else {
        return Ok(None);
    };
    let operation_id = meta_value
        .get("operation_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let reserved_init = operation_id
        .starts_with(crate::internal::secure_direct::v2_runtime::SESSION_INIT_OPERATION_PREFIX);
    let reserved_reply = operation_id
        .starts_with(crate::internal::secure_direct::v2_runtime::SESSION_REPLY_OPERATION_PREFIX);
    if !reserved_init && !reserved_reply {
        return Ok(None);
    }
    let is_init =
        crate::internal::secure_direct::v2_runtime::is_session_init_operation_id(operation_id);
    let is_reply =
        crate::internal::secure_direct::v2_runtime::is_session_reply_operation_id(operation_id);
    if !is_init && !is_reply {
        return Err(crate::ImError::PermissionDenied);
    }
    if meta_value.get("profile").and_then(Value::as_str)
        != Some(anp::direct_e2ee::DIRECT_E2EE_PROFILE_V2)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let expected_content_type = if is_init {
        anp::direct_e2ee::CONTENT_TYPE_DIRECT_INIT_V2
    } else {
        anp::direct_e2ee::CONTENT_TYPE_DIRECT_CIPHER_V2
    };
    if meta_value.get("content_type").and_then(Value::as_str) != Some(expected_content_type) {
        return Err(crate::ImError::PermissionDenied);
    }
    let metadata: anp::direct_e2ee::V2DirectMetadata =
        serde_json::from_value(meta_value.clone()).map_err(|_| crate::ImError::PermissionDenied)?;
    metadata
        .validate()
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let body_value = object
        .get("body")
        .cloned()
        .ok_or(crate::ImError::PermissionDenied)?;
    let body = match metadata.content_type.as_str() {
        anp::direct_e2ee::CONTENT_TYPE_DIRECT_INIT_V2 => {
            let body: anp::direct_e2ee::V2DirectInitBody =
                serde_json::from_value(body_value).map_err(|_| crate::ImError::PermissionDenied)?;
            body.validate()
                .map_err(|_| crate::ImError::PermissionDenied)?;
            anp::direct_e2ee::V2DirectBody::Init(body)
        }
        anp::direct_e2ee::CONTENT_TYPE_DIRECT_CIPHER_V2 => {
            let body: anp::direct_e2ee::V2DirectCipherBody =
                serde_json::from_value(body_value).map_err(|_| crate::ImError::PermissionDenied)?;
            body.validate()
                .map_err(|_| crate::ImError::PermissionDenied)?;
            anp::direct_e2ee::V2DirectBody::Cipher(body)
        }
        _ => return Err(crate::ImError::PermissionDenied),
    };
    Ok(Some((metadata, body)))
}

#[cfg(feature = "sqlite")]
pub(crate) fn is_v2_session_control_projection(message: &Value) -> bool {
    !matches!(parse_v2_session_control(message), Ok(None))
}

fn is_p5_v2_projection_candidate(message: &Value) -> bool {
    message
        .get("meta")
        .or_else(|| message.pointer("/params/meta"))
        .and_then(|meta| meta.get("profile"))
        .and_then(Value::as_str)
        == Some(anp::direct_e2ee::DIRECT_E2EE_PROFILE_V2)
}

fn is_p6_v2_projection_candidate(message: &Value) -> bool {
    message
        .get("meta")
        .or_else(|| message.pointer("/params/meta"))
        .and_then(|meta| meta.get("profile"))
        .and_then(Value::as_str)
        == Some(anp::group_e2ee::GROUP_E2EE_PROFILE_V2)
}

fn p5_v2_wire_projection(message: &Value) -> Value {
    let Some(params) = message.get("params").and_then(Value::as_object) else {
        return message.clone();
    };
    serde_json::json!({
        "meta": params.get("meta").cloned().unwrap_or(Value::Null),
        "body": params.get("body").cloned().unwrap_or(Value::Null),
    })
}

#[cfg(feature = "sqlite")]
fn apply_p5_v2_product_outcome(
    message: &mut Value,
    outcome: crate::internal::secure_direct::v2_product::V2InboundProductOutcome,
) {
    use crate::internal::secure_direct::v2_product::V2InboundProductOutcome;

    match outcome {
        V2InboundProductOutcome::Business(projection) => {
            apply_p5_v2_business_body(message, &projection.body);
            if let Some(object) = message.as_object_mut() {
                object.insert(
                    "id".to_owned(),
                    Value::String(projection.logical_message_id),
                );
                object.insert(
                    "raw_message_id".to_owned(),
                    Value::String(projection.wire_message_id),
                );
                object.insert(
                    "sender_did".to_owned(),
                    Value::String(projection.sender_did),
                );
                object.insert(
                    "sender_device_id".to_owned(),
                    Value::String(projection.sender_device_id),
                );
                object.insert("direction".to_owned(), Value::from(0));
            }
        }
        V2InboundProductOutcome::OwnSync(projection) => {
            apply_p5_v2_business_body(message, &projection.body);
            if let Some(object) = message.as_object_mut() {
                object.insert(
                    "id".to_owned(),
                    Value::String(projection.logical_message_id),
                );
                object.insert(
                    "raw_message_id".to_owned(),
                    Value::String(projection.wire_message_id),
                );
                object.insert(
                    "sender_did".to_owned(),
                    Value::String(projection.original_sender_did),
                );
                object.insert(
                    "sender_device_id".to_owned(),
                    Value::String(projection.original_sender_device_id),
                );
                object.insert(
                    "receiver_did".to_owned(),
                    Value::String(projection.target_did),
                );
                object.insert("direction".to_owned(), Value::from(1));
                object.insert("own_device_sync".to_owned(), Value::Bool(true));
            }
        }
        V2InboundProductOutcome::Replay
        | V2InboundProductOutcome::ConsumedControl
        | V2InboundProductOutcome::SuppressedControl => mark_private_root_control(message),
    }
}

#[cfg(feature = "sqlite")]
fn apply_p5_v2_business_body(
    message: &mut Value,
    body: &crate::internal::secure_direct::v2_product::V2InboundBusinessBody,
) {
    use crate::internal::secure_direct::v2_product::V2InboundBusinessBody;

    let plaintext = match body {
        V2InboundBusinessBody::Text { text, markdown } => json_object([
            ("state", Value::String("decrypted".to_owned())),
            (
                "plaintext",
                serde_json::json!({
                    "application_content_type": if *markdown { "text/markdown" } else { "text/plain" },
                    "text": text,
                }),
            ),
        ]),
        V2InboundBusinessBody::Json { payload } => json_object([
            ("state", Value::String("decrypted".to_owned())),
            (
                "plaintext",
                serde_json::json!({
                    "application_content_type": "application/json",
                    "payload": payload,
                }),
            ),
        ]),
        V2InboundBusinessBody::Attachment { full_manifest } => json_object([
            ("state", Value::String("decrypted".to_owned())),
            (
                "plaintext",
                serde_json::json!({
                    "application_content_type": crate::attachments::manifest::attachment_manifest_content_type(),
                    "payload": full_manifest,
                }),
            ),
        ]),
    };
    crate::internal::secure_direct::incoming::apply_direct_e2ee_processing_result(
        message, &plaintext,
    );
}

#[cfg(feature = "sqlite")]
fn mark_private_root_control(message: &mut Value) {
    let Some(object) = message.as_object_mut() else {
        return;
    };
    object.insert("secure".to_owned(), Value::Bool(true));
    object.insert("secure_control".to_owned(), Value::Bool(true));
    object.insert(
        "type".to_owned(),
        Value::String("secure_control".to_owned()),
    );
    object.insert("content".to_owned(), Value::String(String::new()));
}

fn mark_async_direct_failure(message: &mut Value, warnings: &mut Vec<String>, err: crate::ImError) {
    crate::internal::secure_direct::incoming::apply_direct_e2ee_processing_result(
        message,
        &json_object([("state", Value::String("failed".to_owned()))]),
    );
    if !is_secure_direct_control_like_message(message) {
        warnings.push(format!(
            "Failed to decrypt secure direct message {}: {err}",
            secure_direct_message_id(message)
        ));
    }
}

fn json_object<const N: usize>(entries: [(&str, Value); N]) -> Value {
    Value::Object(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

fn is_direct_e2ee_wire_message(message: &Value) -> bool {
    anp::direct_e2ee::is_direct_e2ee_wire_content_type(&direct_message_content_type(message))
}

fn direct_message_content_type(message: &Value) -> String {
    message
        .as_object()
        .and_then(|object| object.get("content_type"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn compare_secure_direct_message_order(left: &Value, right: &Value) -> std::cmp::Ordering {
    let left_seq = secure_direct_message_server_seq(left).unwrap_or_default();
    let right_seq = secure_direct_message_server_seq(right).unwrap_or_default();
    if left_seq == right_seq {
        return secure_direct_message_id(left).cmp(&secure_direct_message_id(right));
    }
    if left_seq == 0 {
        return std::cmp::Ordering::Greater;
    }
    if right_seq == 0 {
        return std::cmp::Ordering::Less;
    }
    left_seq.cmp(&right_seq)
}

fn secure_direct_message_server_seq(message: &Value) -> Option<i64> {
    match message.as_object()?.get("server_seq")? {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok()))
            .or_else(|| number.as_f64().map(|value| value as i64)),
        Value::String(value) => value.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn secure_direct_message_id(message: &Value) -> String {
    message
        .as_object()
        .and_then(|object| object.get("id").or_else(|| object.get("msg_id")))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn direct_notification_message_id(notification: &Map<String, Value>) -> Option<String> {
    notification
        .get("meta")
        .and_then(Value::as_object)
        .and_then(|meta| meta.get("message_id"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn is_secure_direct_control_like_message(message: &Value) -> bool {
    let content_type = direct_message_content_type(message);
    if content_type == "application/anp-direct-init+json" {
        return true;
    }
    let id = secure_direct_message_id(message);
    id.starts_with("secure-init-") || id.starts_with("ack-")
}

fn compact_secure_direct_warnings(warnings: Vec<String>) -> Vec<String> {
    let mut compact = Vec::new();
    for warning in warnings {
        let warning = warning.trim();
        if warning.is_empty() || compact.iter().any(|known: &String| known == warning) {
            continue;
        }
        compact.push(warning.to_owned());
    }
    compact
}

fn append_secure_direct_warnings(raw: &mut Value, warnings: Vec<String>) {
    if warnings.is_empty() {
        return;
    }
    let Some(object) = raw.as_object_mut() else {
        return;
    };
    let entry = object
        .entry("warnings")
        .or_insert_with(|| Value::Array(Vec::new()));
    if let Value::Array(items) = entry {
        items.extend(warnings.into_iter().map(Value::String));
    }
}

#[cfg(feature = "sqlite")]
fn secure_direct_wire_message_ids(messages: &[Value]) -> Vec<String> {
    messages
        .iter()
        .filter(|message| is_direct_e2ee_wire_message(message))
        .map(secure_direct_message_id)
        .filter(|message_id| !message_id.trim().is_empty())
        .collect()
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
fn apply_cached_secure_direct_messages(
    client: &crate::core::ImClient,
    messages: &mut [Value],
) -> HashSet<usize> {
    let message_ids = secure_direct_wire_message_ids(messages);
    if message_ids.is_empty() {
        return HashSet::new();
    }
    let Ok(connection) = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    ) else {
        return HashSet::new();
    };
    let Ok(records) =
        crate::internal::local_state::messages::list_decrypted_secure_messages_for_owner_identity(
            &connection,
            client.current_identity().id.as_str(),
            &message_ids,
        )
    else {
        return HashSet::new();
    };
    apply_cached_secure_direct_records(messages, records)
}

#[cfg(all(feature = "sqlite", not(any(feature = "blocking", test))))]
fn apply_cached_secure_direct_messages(
    _client: &crate::core::ImClient,
    _messages: &mut [Value],
) -> HashSet<usize> {
    HashSet::new()
}

#[cfg(feature = "sqlite")]
async fn apply_cached_secure_direct_messages_async(
    client: &crate::core::ImClient,
    messages: &mut [Value],
) -> HashSet<usize> {
    let message_ids = secure_direct_wire_message_ids(messages);
    if message_ids.is_empty() {
        return HashSet::new();
    }
    let Ok(db) = client.core_inner().local_state_db().await else {
        return HashSet::new();
    };
    let Ok(records) = db
        .list_decrypted_secure_messages(
            client.current_identity().id.as_str().to_owned(),
            message_ids,
        )
        .await
    else {
        return HashSet::new();
    };
    apply_cached_secure_direct_records(messages, records)
}

#[cfg(feature = "sqlite")]
fn apply_cached_secure_direct_records(
    messages: &mut [Value],
    records: Vec<crate::internal::local_state::messages::MessageRecord>,
) -> HashSet<usize> {
    let mut records_by_wire_id = HashMap::<String, Vec<_>>::new();
    for record in records {
        records_by_wire_id
            .entry(cached_secure_direct_wire_message_id(&record))
            .or_default()
            .push(record);
    }
    let mut applied = HashSet::new();
    for (index, message) in messages.iter_mut().enumerate() {
        let wire_message_id = secure_direct_message_id(message);
        let sender_did = message
            .get("sender_did")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let receiver_did = message
            .get("receiver_did")
            .or_else(|| message.get("recipient_did"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let Some(candidates) = records_by_wire_id.get(&wire_message_id) else {
            continue;
        };
        let record = candidates
            .iter()
            .find(|record| record.sender_did == sender_did && record.receiver_did == receiver_did)
            .or_else(|| {
                let record = (candidates.len() == 1).then(|| &candidates[0])?;
                (is_p5_v2_projection_candidate(message)
                    && record.direction == 1
                    && record.sender_did == record.owner_did
                    && sender_did == record.owner_did
                    && receiver_did == record.owner_did)
                    .then_some(record)
            });
        let Some(record) = record else {
            continue;
        };
        let plaintext = cached_secure_direct_plaintext(record);
        crate::internal::secure_direct::incoming::apply_direct_e2ee_processing_result(
            message,
            &json_object([
                ("state", Value::String("decrypted".to_owned())),
                ("plaintext", plaintext),
            ]),
        );
        if let Some(object) = message.as_object_mut() {
            object.insert("id".to_owned(), Value::String(record.msg_id.clone()));
            object.insert("raw_message_id".to_owned(), Value::String(wire_message_id));
            object.insert(
                "sender_did".to_owned(),
                Value::String(record.sender_did.clone()),
            );
            object.insert(
                "receiver_did".to_owned(),
                Value::String(record.receiver_did.clone()),
            );
            object.insert("direction".to_owned(), Value::from(record.direction));
        }
        applied.insert(index);
    }
    applied
}

#[cfg(feature = "sqlite")]
fn cached_secure_direct_wire_message_id(
    record: &crate::internal::local_state::messages::MessageRecord,
) -> String {
    serde_json::from_str::<Value>(&record.metadata)
        .ok()
        .and_then(|metadata| {
            metadata
                .get("raw_message_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| record.msg_id.clone())
}

#[cfg(feature = "sqlite")]
fn cached_secure_direct_plaintext(
    record: &crate::internal::local_state::messages::MessageRecord,
) -> Value {
    let mut plaintext = serde_json::Map::from_iter([(
        "application_content_type".to_owned(),
        Value::String(record.content_type.clone()),
    )]);
    if record.content_type == "application/json"
        || record.content_type == crate::attachments::manifest::attachment_manifest_content_type()
    {
        if let Ok(payload) = serde_json::from_str::<Value>(&record.content) {
            plaintext.insert("payload".to_owned(), payload);
        }
    } else if record.content_type == "application/octet-stream" {
        plaintext.insert(
            "payload_b64u".to_owned(),
            Value::String(record.content.clone()),
        );
    } else {
        plaintext.insert("text".to_owned(), Value::String(record.content.clone()));
    }
    Value::Object(plaintext)
}

#[cfg(feature = "group-e2ee")]
pub(crate) fn project_group_e2ee_messages(client: &crate::core::ImClient, raw: &mut Value) {
    project_group_e2ee_messages_impl(client, raw, true);
}

#[cfg(feature = "group-e2ee")]
fn consume_group_e2ee_control_messages(client: &crate::core::ImClient, raw: &mut Value) {
    let Some(messages) = raw.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };
    let message_values = std::mem::take(messages);
    let mut retained = Vec::with_capacity(message_values.len());
    let mut warnings = Vec::new();
    for message in message_values {
        if crate::internal::group_e2ee::v2_notice::is_v2_notice_candidate(&message) {
            if client.core_inner().group_e2ee_v2_enabled()
                && crate::internal::group_e2ee::v2_notice::consume_for_client(client, &message)
                    .is_err()
            {
                warnings.push("P6 v2 group control notice was rejected".to_owned());
            }
            continue;
        }
        if crate::internal::group_e2ee::v2_notice::is_explicit_notice_control(&message) {
            continue;
        }
        retained.push(message);
    }
    *messages = retained;
    append_secure_direct_warnings(raw, warnings);
}

#[cfg(not(feature = "group-e2ee"))]
fn consume_group_e2ee_control_messages(_client: &crate::core::ImClient, raw: &mut Value) {
    let Some(messages) = raw.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };
    messages.retain(|message| {
        !is_p6_v2_projection_candidate(message)
            && message.get("method").and_then(Value::as_str)
                != Some(anp::group_e2ee::METHOD_GROUP_NOTICE_V2)
    });
}

#[cfg(feature = "group-e2ee")]
async fn consume_group_e2ee_control_messages_async(
    client: &crate::core::ImClient,
    raw: &mut Value,
) {
    let Some(messages) = raw.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };
    let message_values = std::mem::take(messages);
    let mut retained = Vec::with_capacity(message_values.len());
    let mut warnings = Vec::new();
    for message in message_values {
        if crate::internal::group_e2ee::v2_notice::is_v2_notice_candidate(&message) {
            if client.core_inner().group_e2ee_v2_enabled()
                && crate::internal::group_e2ee::v2_notice::consume_for_client_async(
                    client, &message,
                )
                .await
                .is_err()
            {
                warnings.push("P6 v2 group control notice was rejected".to_owned());
            }
            continue;
        }
        if crate::internal::group_e2ee::v2_notice::is_explicit_notice_control(&message) {
            continue;
        }
        retained.push(message);
    }
    *messages = retained;
    append_secure_direct_warnings(raw, warnings);
}

#[cfg(not(feature = "group-e2ee"))]
async fn consume_group_e2ee_control_messages_async(
    client: &crate::core::ImClient,
    raw: &mut Value,
) {
    consume_group_e2ee_control_messages(client, raw);
}

#[cfg(feature = "group-e2ee")]
fn project_group_e2ee_messages_impl(
    client: &crate::core::ImClient,
    raw: &mut Value,
    redact_attachment_secrets: bool,
) {
    consume_group_e2ee_control_messages(client, raw);
    let Some(messages) = raw.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };
    let message_values = std::mem::take(messages);
    let mut retained = Vec::with_capacity(message_values.len());
    for message in message_values {
        if is_p6_v2_projection_candidate(&message) {
            continue;
        }
        retained.push(message);
    }
    let mut message_values = retained;
    // P6 v2 notices were consumed above. Application decryption remains on the
    // async device-scoped path, so blocking reads must never hand a v2
    // ciphertext to the legacy decoder or ordinary message projection.
    apply_cached_group_e2ee_messages(client, &mut message_values);
    let warnings =
        crate::internal::group_e2ee::incoming::maybe_decrypt_group_e2ee_messages_for_client(
            client,
            &mut message_values,
        );
    cache_attachment_manifests_for_internal_download(client, &message_values);
    if redact_attachment_secrets {
        redact_attachment_manifests_for_public_projection(&mut message_values);
    }
    *messages = message_values;
    append_secure_direct_warnings(raw, warnings);
}

#[cfg(not(feature = "group-e2ee"))]
pub(crate) fn project_group_e2ee_messages(_client: &crate::core::ImClient, _raw: &mut Value) {}

pub(crate) fn project_group_e2ee_messages_for_attachment_download(
    client: &crate::core::ImClient,
    raw: &mut Value,
) {
    #[cfg(feature = "group-e2ee")]
    project_group_e2ee_messages_impl(client, raw, false);
    #[cfg(not(feature = "group-e2ee"))]
    project_group_e2ee_messages(client, raw);
}

#[cfg(feature = "group-e2ee")]
pub(crate) async fn project_group_e2ee_messages_async(
    client: &crate::core::ImClient,
    raw: &mut Value,
) {
    project_group_e2ee_messages_async_impl(client, raw, true).await;
}

#[cfg(feature = "group-e2ee")]
async fn project_group_e2ee_messages_async_impl(
    client: &crate::core::ImClient,
    raw: &mut Value,
    redact_attachment_secrets: bool,
) {
    consume_group_e2ee_control_messages_async(client, raw).await;
    let Some(messages) = raw.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };
    let mut message_values = std::mem::take(messages);
    let mut p6_projected = Vec::with_capacity(message_values.len());
    for mut message in message_values.drain(..) {
        if !is_p6_v2_projection_candidate(&message) {
            p6_projected.push(message);
            continue;
        }
        if !client.core_inner().group_e2ee_v2_enabled() {
            continue;
        }
        if project_p6_v2_incoming_message(client, &mut message)
            .await
            .is_ok()
        {
            p6_projected.push(message);
        }
    }
    message_values = p6_projected;
    apply_cached_group_e2ee_messages_async(client, &mut message_values).await;
    let warnings =
        crate::internal::group_e2ee::incoming::maybe_decrypt_group_e2ee_messages_for_client_async(
            client,
            &mut message_values,
        )
        .await;
    cache_attachment_manifests_for_internal_download_async(client, &message_values).await;
    if redact_attachment_secrets {
        redact_attachment_manifests_for_public_projection(&mut message_values);
    }
    *messages = message_values;
    append_secure_direct_warnings(raw, warnings);
}

#[cfg(feature = "group-e2ee")]
pub(crate) async fn project_p6_v2_incoming_message(
    client: &crate::core::ImClient,
    message: &mut Value,
) -> crate::ImResult<()> {
    let wrapper_shape = message.get("params").is_some();
    let notification = if wrapper_shape {
        message.clone()
    } else {
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": anp::group_e2ee::METHOD_GROUP_INCOMING_V2,
            "params": {
                "meta": message.get("meta").cloned().ok_or(crate::ImError::PermissionDenied)?,
                "body": message.get("body").cloned().ok_or(crate::ImError::PermissionDenied)?,
                "auth": message.get("auth").cloned().ok_or(crate::ImError::PermissionDenied)?,
            }
        })
    };
    let (meta, body, auth) = anp::group_e2ee::parse_group_incoming_notification_v2(&notification)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let sender_document = resolve_direct_sender_document_async(
        client,
        &mut crate::internal::transport::CoreHttpTransport::new(client),
        &meta.sender_did,
    )
    .await?;
    let recipient_device_id = client
        .current_identity()
        .device_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(crate::ImError::PermissionDenied)?
        .to_owned();
    let runtime = crate::internal::group_e2ee::v2_runtime::runtime_for_client(client)?;
    // The Host is not consulted by decrypt_incoming_application; using the
    // production adapter here keeps one product type without adding a second
    // wire implementation.
    let host = crate::internal::group_e2ee::v2_product::RpcGroupE2eeV2Host::new(
        crate::internal::transport::CoreHttpTransport::new(client),
        crate::internal::proof::origin::OriginProofIdentity {
            identity_name: client.current_identity().id.as_str().to_owned(),
            did_document: None,
            key1_private_pem: String::new(),
            verification_method: None,
        },
    );
    let product = crate::internal::group_e2ee::v2_product::GroupE2eeV2Product::new(runtime, host);
    let group_did = body.group_did.clone();
    let group_state_version = body.group_state_version.clone();
    let group_event_seq = body.group_event_seq.clone();
    let accepted_at = body.accepted_at.clone();
    let message_id = meta.message_id.clone();
    let sender_did = meta.sender_did.clone();
    let sender_device_id = meta.sender_device_id.clone();
    let output = product.decrypt_incoming_application(
        crate::internal::group_e2ee::v2_product::V2IncomingApplicationInput {
            recipient_did: client.did().as_str().to_owned(),
            recipient_device_id,
            meta,
            body,
            auth,
            sender_did_document: sender_document,
            now: crate::internal::wire::common::now_rfc3339(),
            draft_extension_negotiated: true,
            request_id: format!(
                "p6-v2-decrypt-{}",
                crate::internal::wire::common::generate_operation_id()
            ),
        },
    )?;
    let plaintext = output.application_plaintext;
    if wrapper_shape {
        *message = Value::Object(Map::new());
    }
    let object = message
        .as_object_mut()
        .ok_or(crate::ImError::PermissionDenied)?;
    object.remove("meta");
    object.remove("body");
    object.remove("auth");
    object.insert("id".to_owned(), Value::String(message_id.clone()));
    object.insert("message_id".to_owned(), Value::String(message_id));
    object.insert("sender_did".to_owned(), Value::String(sender_did));
    object.insert(
        "sender_device_id".to_owned(),
        Value::String(sender_device_id),
    );
    object.insert("group_did".to_owned(), Value::String(group_did));
    object.insert(
        "group_state_version".to_owned(),
        Value::String(group_state_version),
    );
    object.insert("group_event_seq".to_owned(), Value::String(group_event_seq));
    object.insert("accepted_at".to_owned(), Value::String(accepted_at));
    object.insert("direction".to_owned(), Value::from(0));
    object.insert("secure".to_owned(), Value::Bool(true));
    object.insert(
        "decryption_state".to_owned(),
        Value::String("decrypted".to_owned()),
    );
    object.insert(
        "content_type".to_owned(),
        Value::String(plaintext.application_content_type.clone()),
    );
    if let Some(text) = plaintext.text {
        object.insert("content".to_owned(), Value::String(text));
        object.insert("type".to_owned(), Value::String("text".to_owned()));
    } else if let Some(payload) = plaintext.payload {
        let message_type = if plaintext.application_content_type
            == crate::attachments::manifest::attachment_manifest_content_type()
        {
            "attachment_manifest"
        } else {
            "json"
        };
        object.insert("content".to_owned(), payload);
        object.insert("type".to_owned(), Value::String(message_type.to_owned()));
    } else if let Some(payload_b64u) = plaintext.payload_b64u {
        object.insert("content".to_owned(), Value::String(payload_b64u));
        object.insert("type".to_owned(), Value::String("binary".to_owned()));
    } else {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

#[cfg(feature = "group-e2ee")]
pub(crate) async fn normalize_p6_v2_realtime_incoming(
    client: &crate::core::ImClient,
    notification: &Value,
) -> crate::ImResult<Value> {
    let params = notification
        .get("params")
        .and_then(Value::as_object)
        .ok_or(crate::ImError::PermissionDenied)?;
    let mut message = serde_json::json!({
        "meta": params.get("meta").cloned().ok_or(crate::ImError::PermissionDenied)?,
        "body": params.get("body").cloned().ok_or(crate::ImError::PermissionDenied)?,
        "auth": params.get("auth").cloned().ok_or(crate::ImError::PermissionDenied)?,
    });
    project_p6_v2_incoming_message(client, &mut message).await?;
    let object = message
        .as_object()
        .ok_or(crate::ImError::PermissionDenied)?;
    let content_type = object
        .get("content_type")
        .and_then(Value::as_str)
        .ok_or(crate::ImError::PermissionDenied)?;
    let content = object
        .get("content")
        .cloned()
        .ok_or(crate::ImError::PermissionDenied)?;
    let body_content = if content_type == "text/plain" || content_type == "text/markdown" {
        serde_json::json!({"text": content})
    } else {
        let payload =
            if content_type == crate::attachments::manifest::attachment_manifest_content_type() {
                crate::attachments::manifest::redact_attachment_manifest(&content)
            } else {
                content
            };
        serde_json::json!({"payload": payload})
    };
    let mut body = body_content
        .as_object()
        .cloned()
        .ok_or(crate::ImError::PermissionDenied)?;
    for key in [
        "group_did",
        "group_state_version",
        "group_event_seq",
        "accepted_at",
    ] {
        if let Some(value) = object.get(key).cloned() {
            body.insert(key.to_owned(), value);
        }
    }
    Ok(serde_json::json!({
        "jsonrpc": "2.0",
        "method": "group.incoming",
        "params": {
            "meta": {
                "profile": anp::group_e2ee::GROUP_E2EE_PROFILE_V2,
                "security_profile": anp::group_e2ee::GROUP_E2EE_SECURITY_PROFILE_V2,
                "sender_did": object.get("sender_did").cloned().unwrap_or(Value::Null),
                "sender_device_id": object.get("sender_device_id").cloned().unwrap_or(Value::Null),
                "target": {"kind": "agent", "did": client.did().as_str()},
                "message_id": object.get("message_id").cloned().unwrap_or(Value::Null),
                "content_type": content_type,
            },
            "body": body,
            "secure": true,
            "secure_state": "decrypted"
        }
    }))
}

#[cfg(not(feature = "group-e2ee"))]
pub(crate) async fn project_group_e2ee_messages_async(
    _client: &crate::core::ImClient,
    _raw: &mut Value,
) {
}

pub(crate) async fn project_group_e2ee_messages_for_attachment_download_async(
    client: &crate::core::ImClient,
    raw: &mut Value,
) {
    #[cfg(feature = "group-e2ee")]
    project_group_e2ee_messages_async_impl(client, raw, false).await;
    #[cfg(not(feature = "group-e2ee"))]
    project_group_e2ee_messages_async(client, raw).await;
}

#[cfg(all(feature = "sqlite", feature = "group-e2ee"))]
fn group_e2ee_wire_message_ids(messages: &[Value]) -> Vec<String> {
    messages
        .iter()
        .filter(|message| crate::internal::group_e2ee::incoming::is_group_e2ee_message(message))
        .map(group_e2ee_message_cache_id)
        .filter(|message_id| !message_id.is_empty())
        .collect()
}

#[cfg(all(feature = "sqlite", feature = "group-e2ee"))]
fn group_e2ee_message_cache_id(message: &Value) -> String {
    let group_did = first_non_empty_owned([
        string_value(message.get("group_did")),
        string_value(message.get("group")),
    ]);
    message_identity(message, group_did.as_deref())
}

#[cfg(all(
    feature = "sqlite",
    feature = "group-e2ee",
    any(feature = "blocking", test)
))]
pub(crate) fn apply_cached_group_e2ee_messages(
    client: &crate::core::ImClient,
    messages: &mut [Value],
) {
    let message_ids = group_e2ee_wire_message_ids(messages);
    if message_ids.is_empty() {
        return;
    }
    let Ok(connection) = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    ) else {
        return;
    };
    let Ok(records) =
        crate::internal::local_state::messages::list_decrypted_secure_messages_for_owner_identity(
            &connection,
            client.current_identity().id.as_str(),
            &message_ids,
        )
    else {
        return;
    };
    apply_cached_group_e2ee_records(messages, records);
}

#[cfg(all(
    feature = "sqlite",
    feature = "group-e2ee",
    not(any(feature = "blocking", test))
))]
pub(crate) fn apply_cached_group_e2ee_messages(
    _client: &crate::core::ImClient,
    _messages: &mut [Value],
) {
}

#[cfg(all(feature = "sqlite", feature = "group-e2ee"))]
pub(crate) async fn apply_cached_group_e2ee_messages_async(
    client: &crate::core::ImClient,
    messages: &mut [Value],
) {
    let message_ids = group_e2ee_wire_message_ids(messages);
    if message_ids.is_empty() {
        return;
    }
    let Ok(db) = client.core_inner().local_state_db().await else {
        return;
    };
    let Ok(records) = db
        .list_decrypted_secure_messages(
            client.current_identity().id.as_str().to_owned(),
            message_ids,
        )
        .await
    else {
        return;
    };
    apply_cached_group_e2ee_records(messages, records);
}

#[cfg(all(feature = "group-e2ee", not(feature = "sqlite")))]
pub(crate) async fn apply_cached_group_e2ee_messages_async(
    _client: &crate::core::ImClient,
    _messages: &mut [Value],
) {
}

#[cfg(all(feature = "sqlite", feature = "group-e2ee"))]
fn apply_cached_group_e2ee_records(
    messages: &mut [Value],
    records: Vec<crate::internal::local_state::messages::MessageRecord>,
) {
    let records = records
        .into_iter()
        .map(|record| (record.msg_id.clone(), record))
        .collect::<HashMap<_, _>>();
    for message in messages {
        let message_id = group_e2ee_message_cache_id(message);
        let Some(record) = records.get(&message_id) else {
            continue;
        };
        let _ = crate::internal::group_e2ee::incoming::apply_cached_group_plaintext(
            message,
            &record.content_type,
            &record.content,
        );
    }
}

pub(crate) fn cache_attachment_manifests_for_internal_download(
    client: &crate::core::ImClient,
    messages: &[Value],
) {
    #[cfg(feature = "sqlite")]
    {
        let records = attachment_manifest_cache_records(client, messages);
        if records.is_empty() {
            return;
        }
        let Ok(connection) = crate::internal::local_state::open_writable(
            &client.core_inner().sdk_paths().local_state.sqlite_path,
        ) else {
            return;
        };
        for record in records {
            let _ =
                crate::internal::local_state::attachment_manifest_cache::upsert_attachment_manifest_cache(
                    &connection,
                    &record,
                );
        }
    }
    #[cfg(not(feature = "sqlite"))]
    {
        let _ = (client, messages);
    }
}

pub(crate) async fn cache_attachment_manifests_for_internal_download_async(
    client: &crate::core::ImClient,
    messages: &[Value],
) {
    #[cfg(feature = "sqlite")]
    {
        let records = attachment_manifest_cache_records(client, messages);
        if records.is_empty() {
            return;
        }
        let sqlite_path = client
            .core_inner()
            .sdk_paths()
            .local_state
            .sqlite_path
            .clone();
        let _ = crate::internal::runtime::worker::run_blocking(move || {
            let connection = crate::internal::local_state::open_writable(&sqlite_path)?;
            for record in records {
                crate::internal::local_state::attachment_manifest_cache::upsert_attachment_manifest_cache(
                    &connection,
                    &record,
                )?;
            }
            Ok::<(), crate::ImError>(())
        })
        .await;
    }
    #[cfg(not(feature = "sqlite"))]
    {
        let _ = (client, messages);
    }
}

#[cfg(feature = "sqlite")]
fn attachment_manifest_cache_records(
    client: &crate::core::ImClient,
    messages: &[Value],
) -> Vec<crate::internal::local_state::attachment_manifest_cache::AttachmentManifestCacheRecord> {
    messages
        .iter()
        .filter_map(|message| attachment_manifest_cache_record(client, message))
        .collect()
}

#[cfg(feature = "sqlite")]
fn attachment_manifest_cache_record(
    client: &crate::core::ImClient,
    message: &Value,
) -> Option<crate::internal::local_state::attachment_manifest_cache::AttachmentManifestCacheRecord>
{
    let object = message.as_object()?;
    if object.get("content_type").and_then(Value::as_str)
        != Some(crate::attachments::manifest::attachment_manifest_content_type())
    {
        return None;
    }
    let content = decoded_attachment_manifest_content_for_cache(object.get("content")?)?;
    if !attachment_manifest_contains_object_secrets(&content) {
        return None;
    }
    let group_did = first_non_empty_owned([
        string_value(object.get("group_did")),
        string_value(object.get("group")),
    ]);
    let (thread_kind, thread_id) = if let Some(group_did) = group_did {
        ("group", group_did)
    } else {
        let sender_did = string_value(object.get("sender_did"));
        let receiver_did = string_value(object.get("receiver_did"));
        let peer_did =
            direct_peer_did_for_message(client.did().as_str(), &sender_did, &receiver_did)
                .trim()
                .to_owned();
        if peer_did.is_empty() {
            return None;
        }
        ("direct", peer_did)
    };
    let message_id = attachment_manifest_cache_message_id(object, &thread_id)?;
    Some(
        crate::internal::local_state::attachment_manifest_cache::AttachmentManifestCacheRecord {
            owner_identity_id: client.current_identity().id.as_str().to_owned(),
            owner_did: client.did().as_str().to_owned(),
            thread_kind: thread_kind.to_owned(),
            thread_id,
            message_id,
            sender_did: string_value(object.get("sender_did")),
            message_security_profile: secure_message_security_profile(object),
            content: serde_json::to_string(&content).ok()?,
            stored_at: first_non_empty_owned([
                string_value(object.get("stored_at")),
                string_value(object.get("received_at")),
                string_value(object.get("sent_at")),
                string_value(object.get("accepted_at")),
            ])
            .unwrap_or_default()
            .to_owned(),
        },
    )
}

#[cfg(feature = "sqlite")]
fn decoded_attachment_manifest_content_for_cache(content: &Value) -> Option<Value> {
    match content {
        Value::String(text) => serde_json::from_str::<Value>(text).ok(),
        value if value.is_object() => Some(value.clone()),
        _ => None,
    }
}

#[cfg(feature = "sqlite")]
fn attachment_manifest_contains_object_secrets(manifest: &Value) -> bool {
    manifest
        .get("attachments")
        .and_then(Value::as_array)
        .map(|attachments| {
            attachments.iter().any(|attachment| {
                let encryption_info = attachment.get("encryption_info").and_then(Value::as_object);
                encryption_info
                    .and_then(|info| info.get("object_key_b64u"))
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .is_some()
                    && encryption_info
                        .and_then(|info| info.get("nonce_b64u"))
                        .and_then(Value::as_str)
                        .filter(|value| !value.trim().is_empty())
                        .is_some()
            })
        })
        .unwrap_or(false)
}

#[cfg(feature = "sqlite")]
fn attachment_manifest_cache_message_id(
    object: &serde_json::Map<String, Value>,
    group_did: &str,
) -> Option<String> {
    first_non_empty_owned([
        string_value(object.get("id")),
        string_value(object.get("message_id")),
        string_value(object.get("msg_id")),
        string_value(object.get("client_msg_id")),
    ])
    .or_else(|| {
        let group_event_seq = string_or_number_value(object.get("group_event_seq"));
        (!group_did.trim().is_empty() && !group_event_seq.trim().is_empty())
            .then(|| format!("{}:{}", group_did.trim(), group_event_seq.trim()))
    })
}

#[cfg(feature = "sqlite")]
fn first_non_empty_owned(values: impl IntoIterator<Item = String>) -> Option<String> {
    values
        .into_iter()
        .map(|value| value.trim().to_owned())
        .find(|value| !value.is_empty())
}

pub(crate) fn redact_attachment_manifests_for_public_projection(messages: &mut [Value]) {
    for message in messages {
        let content_type = message.get("content_type").and_then(Value::as_str);
        if content_type != Some(crate::attachments::manifest::attachment_manifest_content_type()) {
            continue;
        }
        let Some(object) = message.as_object_mut() else {
            continue;
        };
        let Some(content) = object.get("content").cloned() else {
            continue;
        };
        object.insert(
            "content".to_owned(),
            redact_attachment_manifest_content(content),
        );
    }
}

fn redact_attachment_manifest_content(content: Value) -> Value {
    match content {
        Value::String(text) => match serde_json::from_str::<Value>(&text) {
            Ok(value) => crate::attachments::manifest::redact_attachment_manifest(&value),
            Err(_) => Value::String(text),
        },
        value => crate::attachments::manifest::redact_attachment_manifest(&value),
    }
}

fn message_from_value(
    client: &crate::core::ImClient,
    value: &Value,
    fallback_group: Option<&crate::ids::GroupRef>,
) -> crate::ImResult<Option<crate::messages::Message>> {
    let Some(object) = value.as_object() else {
        return Ok(None);
    };
    let sender_did = string_value(object.get("sender_did"));
    let receiver_did = string_value(object.get("receiver_did"));
    let mut group_did = string_value(object.get("group_did"));
    if group_did.trim().is_empty() {
        if let Some(group) = fallback_group {
            group_did = group.as_str().to_string();
        }
    }
    let id = message_identity(
        value,
        (!group_did.trim().is_empty()).then_some(group_did.as_str()),
    );
    if id.trim().is_empty() {
        return Ok(None);
    }
    let retry_target = if group_did.trim().is_empty() {
        Some(crate::internal::message_runtime::state::MessageRetryTarget::DirectText)
    } else {
        Some(crate::internal::message_runtime::state::MessageRetryTarget::GroupText)
    };
    let metadata = message_metadata_from_object(object, &id, retry_target);
    let thread = if !group_did.trim().is_empty() {
        crate::messages::ThreadRef::Group(crate::ids::GroupRef::parse(&group_did)?)
    } else if let Some(thread) =
        crate::internal::message_runtime::local_projection::scoped_direct_thread_ref_from_metadata(
            &metadata,
        )
    {
        thread
    } else {
        let peer = direct_peer_did_for_message(client.did().as_str(), &sender_did, &receiver_did);
        crate::messages::ThreadRef::Direct(crate::ids::PeerRef::parse(peer, "")?)
    };
    Ok(Some(crate::messages::Message {
        id: crate::ids::MessageId::parse(id)?,
        thread,
        direction: message_direction(value),
        sender: crate::ids::PeerRef::parse(non_empty_or(&sender_did, "did:unknown:sender"), "")?,
        receiver: (!receiver_did.trim().is_empty())
            .then(|| crate::ids::PeerRef::parse(&receiver_did, ""))
            .transpose()?,
        group: (!group_did.trim().is_empty())
            .then(|| crate::ids::GroupRef::parse(&group_did))
            .transpose()?,
        body: message_body(value),
        sent_at: message_sent_at(object),
        received_at: Some(string_value(object.get("received_at")))
            .filter(|value| !value.trim().is_empty()),
        metadata,
    }))
}

fn direct_peer_did_for_message<'a>(
    owner_did: &str,
    sender_did: &'a str,
    receiver_did: &'a str,
) -> &'a str {
    let owner = owner_did.trim();
    let sender = sender_did.trim();
    let receiver = receiver_did.trim();
    if !sender.is_empty() && sender != owner {
        return sender_did;
    }
    if !receiver.is_empty() && receiver != owner {
        return receiver_did;
    }
    if !receiver.is_empty() {
        return receiver_did;
    }
    sender_did
}

fn message_metadata_from_object(
    object: &serde_json::Map<String, Value>,
    message_id: &str,
    retry_target: Option<crate::internal::message_runtime::state::MessageRetryTarget>,
) -> crate::messages::MessageMetadata {
    let metadata_json = metadata_projection_json(object, message_id);
    let send_state = crate::internal::message_runtime::state::send_state_from_metadata(
        &metadata_json,
        message_id,
    );
    let retry_plan = crate::internal::message_runtime::state::retry_plan_from_metadata(
        &metadata_json,
        send_state.as_ref(),
        retry_target,
    );
    let content_type =
        Some(string_value(object.get("content_type"))).filter(|value| !value.trim().is_empty());
    let mut attributes =
        metadata_attributes_from_object(object, message_id, content_type.as_deref());
    attributes.extend(secure_message_attributes(object));
    crate::messages::MessageMetadata {
        operation_id: Some(string_value(object.get("operation_id")))
            .filter(|value| !value.trim().is_empty()),
        delivery_state: Some(string_value(object.get("delivery_state")))
            .filter(|value| !value.trim().is_empty()),
        send_state,
        retry_plan,
        server_sequence: i64_value(object.get("server_seq"))
            .or_else(|| i64_value(object.get("sequence")))
            .or_else(|| i64_value(object.get("group_event_seq"))),
        content_type: content_type.clone(),
        conversation_identity: None,
        attributes,
    }
}

fn metadata_projection_json(object: &serde_json::Map<String, Value>, message_id: &str) -> String {
    let mut metadata = serde_json::Map::new();
    metadata.insert(
        "message_id".to_string(),
        Value::String(message_id.to_string()),
    );
    for key in [
        "operation_id",
        "delivery_state",
        "failure_reason",
        "send_state_updated_at",
        "accepted_at",
        "send_state",
        "retry_plan",
    ] {
        if let Some(value) = object.get(key) {
            metadata.insert(key.to_string(), value.clone());
        }
    }
    Value::Object(metadata).to_string()
}

fn message_sent_at(object: &serde_json::Map<String, Value>) -> Option<String> {
    [
        object.get("sent_at"),
        object.get("accepted_at"),
        object.get("created_at"),
    ]
    .into_iter()
    .map(string_or_number_value)
    .find(|value| !value.trim().is_empty())
}

fn metadata_attributes_from_object(
    object: &serde_json::Map<String, Value>,
    message_id: &str,
    content_type: Option<&str>,
) -> Vec<crate::messages::MessageMetadataAttribute> {
    let mut attributes = raw_content_attributes(object.get("content"), content_type);
    if let Some(is_read) = bool_value(object.get("is_read")) {
        attributes.push(crate::messages::MessageMetadataAttribute {
            key: "is_read".to_string(),
            value: is_read.to_string(),
        });
    }
    let raw_message_id = if object
        .get("meta")
        .or_else(|| object.get("params").and_then(|params| params.get("meta")))
        .and_then(Value::as_object)
        .and_then(|meta| meta.get("profile"))
        .and_then(Value::as_str)
        == Some(anp::direct_e2ee::DIRECT_E2EE_PROFILE_V2)
    {
        string_or_number_value(object.get("raw_message_id"))
    } else {
        String::new()
    };
    let raw_message_id = if raw_message_id.trim().is_empty() {
        raw_message_identity(object)
    } else {
        raw_message_id
    };
    if !raw_message_id.trim().is_empty() && raw_message_id != message_id {
        attributes.push(crate::messages::MessageMetadataAttribute {
            key: "raw_message_id".to_string(),
            value: raw_message_id,
        });
    }
    let group_event_seq = string_or_number_value(object.get("group_event_seq"));
    if !group_event_seq.trim().is_empty() {
        attributes.push(crate::messages::MessageMetadataAttribute {
            key: "group_event_seq".to_string(),
            value: group_event_seq,
        });
    }
    for key in [
        "peer_user_id",
        "peer_full_handle",
        "peer_current_did",
        "resolved_target_did",
        "target_handle",
    ] {
        let value = string_value(object.get(key));
        if !value.trim().is_empty() {
            attributes.push(crate::messages::MessageMetadataAttribute {
                key: key.to_string(),
                value,
            });
        }
    }
    attributes
}

fn metadata_attribute<'a>(
    metadata: &'a crate::messages::MessageMetadata,
    key: &str,
) -> Option<&'a str> {
    metadata
        .attributes
        .iter()
        .find(|attribute| attribute.key == key)
        .map(|attribute| attribute.value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn message_identity(message: &Value, group_did: Option<&str>) -> String {
    let Some(object) = message.as_object() else {
        return String::new();
    };
    let group_event_seq = string_or_number_value(object.get("group_event_seq"));
    if let Some(group_did) = group_did.filter(|_| !group_event_seq.trim().is_empty()) {
        if !group_did.trim().is_empty() {
            return format!("{}:{}", group_did.trim(), group_event_seq.trim());
        }
    }
    raw_message_identity(object)
}

fn raw_message_identity(object: &serde_json::Map<String, Value>) -> String {
    string_or_number_value(
        object
            .get("id")
            .or_else(|| object.get("message_id"))
            .or_else(|| object.get("msg_id"))
            .or_else(|| object.get("client_msg_id")),
    )
}

fn message_direction(value: &Value) -> crate::messages::MessageDirection {
    let direction = value.get("direction").and_then(Value::as_i64).or_else(|| {
        value
            .get("direction")
            .and_then(Value::as_str)
            .and_then(|value| value.parse().ok())
    });
    match direction {
        Some(1) => crate::messages::MessageDirection::Outgoing,
        Some(0) => crate::messages::MessageDirection::Incoming,
        _ => crate::messages::MessageDirection::Unknown,
    }
}

fn message_body(value: &Value) -> crate::messages::MessageBodyView {
    let content_type = value
        .get("content_type")
        .and_then(Value::as_str)
        .map(str::to_string);
    let Some(content) = value.get("content") else {
        return crate::messages::MessageBodyView::Unsupported { content_type };
    };
    if content_type.as_deref() == Some("application/json")
        || content_type.as_deref()
            == Some(crate::attachments::manifest::attachment_manifest_content_type())
    {
        let payload = match content {
            Value::String(value) => match serde_json::from_str::<Value>(value) {
                Ok(value) => value,
                Err(_) => {
                    return crate::messages::MessageBodyView::Unsupported { content_type };
                }
            },
            value => value.clone(),
        };
        if payload.is_object() {
            return crate::messages::MessageBodyView::Payload { payload };
        }
        return crate::messages::MessageBodyView::Unsupported { content_type };
    }
    let text = match content {
        Value::String(value) => value.clone(),
        value => serde_json::to_string(value).unwrap_or_default(),
    };
    let kind = match content_type.as_deref() {
        Some("text/markdown") => crate::messages::MessageKind::Markdown,
        Some("text/plain") | None | Some("") => crate::messages::MessageKind::Text,
        _ => return crate::messages::MessageBodyView::Unsupported { content_type },
    };
    crate::messages::MessageBodyView::Text { text, kind }
}

fn raw_content_attributes(
    content: Option<&Value>,
    content_type: Option<&str>,
) -> Vec<crate::messages::MessageMetadataAttribute> {
    let Some(content) = content else {
        return Vec::new();
    };
    let Some(content_type) = content_type
        .map(str::trim)
        .filter(|content_type| !content_type.is_empty())
    else {
        return Vec::new();
    };
    if content_type != crate::attachments::manifest::attachment_manifest_content_type() {
        return Vec::new();
    }
    if content.is_null() {
        return Vec::new();
    }
    let value = match content {
        Value::String(text) => text.clone(),
        value => serde_json::to_string(value).unwrap_or_default(),
    };
    if value.trim().is_empty() {
        return Vec::new();
    }
    vec![crate::messages::MessageMetadataAttribute {
        key: "raw_content".to_string(),
        value,
    }]
}

fn secure_message_attributes(
    object: &serde_json::Map<String, Value>,
) -> Vec<crate::messages::MessageMetadataAttribute> {
    if !object
        .get("secure")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Vec::new();
    }
    let mut attributes = vec![crate::messages::MessageMetadataAttribute {
        key: "security".to_owned(),
        value: secure_message_security_profile(object),
    }];
    for key in ["decryption_state", "secure_wire_content_type"] {
        let value = string_value(object.get(key));
        if !value.trim().is_empty() {
            attributes.push(crate::messages::MessageMetadataAttribute {
                key: key.to_owned(),
                value,
            });
        }
    }
    attributes
}

fn secure_message_security_profile(object: &serde_json::Map<String, Value>) -> String {
    for key in ["message_security_profile", "security_profile", "security"] {
        let value = string_value(object.get(key));
        if !value.trim().is_empty() {
            return normalize_secure_message_security_profile(&value);
        }
    }
    if string_value(object.get("group_did")).trim().is_empty() {
        "direct-e2ee".to_owned()
    } else {
        "group-e2ee".to_owned()
    }
}

fn normalize_secure_message_security_profile(value: &str) -> String {
    match value.trim() {
        "secure-direct" | "direct" | "direct_e2ee" | "e2ee" => "direct-e2ee".to_owned(),
        "group" | "group_e2ee" => "group-e2ee".to_owned(),
        value => value.to_owned(),
    }
}

fn string_value(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn bool_value(value: Option<&Value>) -> Option<bool> {
    match value {
        Some(Value::Bool(value)) => Some(*value),
        Some(Value::Number(value)) => value.as_i64().map(|value| value != 0),
        Some(Value::String(value)) => match value.trim() {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn string_or_number_value(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        _ => String::new(),
    }
}

fn i64_value(value: Option<&Value>) -> Option<i64> {
    match value {
        Some(Value::Number(number)) => number
            .as_i64()
            .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok()))
            .or_else(|| number.as_f64().map(|value| value as i64)),
        Some(Value::String(value)) => value.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn non_empty_or<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.trim().is_empty() {
        fallback
    } else {
        value
    }
}

#[cfg(test)]
mod tests;
