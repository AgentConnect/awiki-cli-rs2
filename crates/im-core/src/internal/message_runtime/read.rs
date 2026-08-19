use std::collections::{HashMap, HashSet};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde_json::{json, Map, Value};
use sha2::{Digest as _, Sha256};
use zeroize::Zeroizing;

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
        Ok(direct)
    }

    fn direct_inbox(
        &mut self,
        query: crate::messages::InboxQuery,
    ) -> crate::ImResult<ReadPageResult> {
        let limit = page_limit(query.limit, 20);
        let delegated =
            delegated_inbox_context(self.client, query.inbox_history_options.as_ref(), limit)?;
        if delegated.is_none() {
            self.session_provider
                .ensure_session(crate::auth::AuthScope::Messaging)?;
        }
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
        #[cfg(feature = "sqlite")]
        if delegated.is_none() {
            let _ =
                consume_system_notifications(self.client, &mut raw, &mut self.directory_transport);
        }
        let mut p5_provenance = if delegated.is_some() {
            filter_delegated_e2ee_messages(&mut raw);
            DirectP5ProjectionProvenance::default()
        } else {
            consume_group_e2ee_control_messages(self.client, &mut raw);
            project_secure_direct_messages(self.client, &mut raw, &mut self.directory_transport)
        };
        annotate_direct_peer_scopes(
            self.client,
            &mut raw,
            &mut self.directory_transport,
            None,
            None,
            Some(&mut p5_provenance),
        );
        let mut page = page_from_raw(self.client, &raw, query.limit)?;
        page.items.retain(|message| message.group.is_none());
        page.has_more |= dedupe_and_truncate_messages(&mut page.items, query.limit);
        persist_projection_best_effort(self.client, &page.items, &p5_provenance);
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
            None,
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
            let params = crate::internal::wire::group::build_group_messages_rpc_params_for_client(
                self.client,
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
            project_group_e2ee_messages_for_group(self.client, &mut group_raw, group.as_str());
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
        persist_projection_best_effort(
            self.client,
            &page.items,
            &DirectP5ProjectionProvenance::default(),
        );
        Ok(ReadPageResult { page, raw })
    }

    pub(crate) fn history(mut self, input: HistoryRead) -> crate::ImResult<ReadPageResult> {
        match input.thread {
            crate::messages::ThreadRef::Direct(peer) => {
                let peer = direct_thread(peer, input.resolved_peer_did)?;
                let delegated = delegated_inbox_context(
                    self.client,
                    input.query.inbox_history_options.as_ref(),
                    page_limit(input.query.limit, 50),
                )?;
                if delegated.is_none() {
                    self.session_provider
                        .ensure_session(crate::auth::AuthScope::Messaging)?;
                }
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
                let mut p5_provenance = if delegated.is_some() {
                    filter_delegated_e2ee_messages(&mut raw);
                    DirectP5ProjectionProvenance::default()
                } else {
                    consume_group_e2ee_control_messages(self.client, &mut raw);
                    project_secure_direct_messages_for_peer(
                        self.client,
                        &mut raw,
                        &mut self.directory_transport,
                        &peer.resolved_did,
                    )
                };
                retain_direct_messages_for_expected_peer(
                    self.client,
                    &mut raw,
                    &peer.resolved_did,
                    &mut p5_provenance,
                );
                annotate_direct_peer_scopes(
                    self.client,
                    &mut raw,
                    &mut self.directory_transport,
                    input.peer_scope.as_ref(),
                    Some(&peer.resolved_did),
                    Some(&mut p5_provenance),
                );
                let page = page_from_raw(self.client, &raw, input.query.limit)?;
                reject_stalled_scoped_direct_page(&raw, page.items.len())?;
                persist_projection_best_effort(self.client, &page.items, &p5_provenance);
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
                let params =
                    crate::internal::wire::group::build_group_messages_rpc_params_for_client(
                        self.client,
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
                project_group_e2ee_messages_for_group(self.client, &mut raw, group.as_str());
                let mut page =
                    page_from_raw_with_group(self.client, &raw, input.query.limit, Some(&group))?;
                persist_projection_best_effort(
                    self.client,
                    &page.items,
                    &DirectP5ProjectionProvenance::default(),
                );
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
        Ok(direct)
    }

    async fn direct_inbox_async(
        &mut self,
        query: crate::messages::InboxQuery,
    ) -> crate::ImResult<ReadPageResult> {
        let limit = page_limit(query.limit, 20);
        let delegated =
            delegated_inbox_context_async(self.client, query.inbox_history_options.as_ref(), limit)
                .await?;
        if delegated.is_none() {
            self.session_provider
                .ensure_session(crate::auth::AuthScope::Messaging)
                .await?;
        }
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
        #[cfg(feature = "sqlite")]
        if delegated.is_none() {
            let _ = consume_system_notifications_async(
                self.client,
                &mut raw,
                &mut self.directory_transport,
            )
            .await;
        }
        let mut p5_provenance = if delegated.is_some() {
            filter_delegated_e2ee_messages(&mut raw);
            DirectP5ProjectionProvenance::default()
        } else {
            consume_group_e2ee_control_messages_async(self.client, &mut raw).await;
            project_secure_direct_messages_async(
                self.client,
                &mut raw,
                &mut self.directory_transport,
            )
            .await
        };
        annotate_direct_peer_scopes_async(
            self.client,
            &mut raw,
            &mut self.directory_transport,
            None,
            None,
            Some(&mut p5_provenance),
        )
        .await;
        let mut page = page_from_raw(self.client, &raw, query.limit)?;
        page.items.retain(|message| message.group.is_none());
        page.has_more |= dedupe_and_truncate_messages(&mut page.items, query.limit);
        persist_projection_best_effort_async(self.client, &page.items, &p5_provenance).await;
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
            None,
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
            let params = crate::internal::wire::group::build_group_messages_rpc_params_for_client(
                self.client,
                group.as_str(),
                limit,
                None,
                0,
            )?;
            let mut group_raw = self
                .transport
                .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "group.list_messages", params)
                .await?;
            project_group_e2ee_messages_for_group_async(
                self.client,
                &mut group_raw,
                group.as_str(),
            )
            .await;
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
        persist_projection_best_effort_async(
            self.client,
            &page.items,
            &DirectP5ProjectionProvenance::default(),
        )
        .await;
        Ok(ReadPageResult { page, raw })
    }

    pub(crate) async fn history_async(
        mut self,
        input: HistoryRead,
    ) -> crate::ImResult<ReadPageResult> {
        match input.thread {
            crate::messages::ThreadRef::Direct(peer) => {
                let peer = direct_thread(peer, input.resolved_peer_did)?;
                let delegated = delegated_inbox_context_async(
                    self.client,
                    input.query.inbox_history_options.as_ref(),
                    page_limit(input.query.limit, 50),
                )
                .await?;
                if delegated.is_none() {
                    self.session_provider
                        .ensure_session(crate::auth::AuthScope::Messaging)
                        .await?;
                }
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
                let mut p5_provenance = if delegated.is_some() {
                    filter_delegated_e2ee_messages(&mut raw);
                    DirectP5ProjectionProvenance::default()
                } else {
                    consume_group_e2ee_control_messages_async(self.client, &mut raw).await;
                    project_secure_direct_messages_async_for_peer(
                        self.client,
                        &mut raw,
                        &mut self.directory_transport,
                        &peer.resolved_did,
                    )
                    .await
                };
                retain_direct_messages_for_expected_peer(
                    self.client,
                    &mut raw,
                    &peer.resolved_did,
                    &mut p5_provenance,
                );
                annotate_direct_peer_scopes_async(
                    self.client,
                    &mut raw,
                    &mut self.directory_transport,
                    input.peer_scope.as_ref(),
                    Some(&peer.resolved_did),
                    Some(&mut p5_provenance),
                )
                .await;
                let page = page_from_raw(self.client, &raw, input.query.limit)?;
                reject_stalled_scoped_direct_page(&raw, page.items.len())?;
                persist_projection_best_effort_async(self.client, &page.items, &p5_provenance)
                    .await;
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
                let params =
                    crate::internal::wire::group::build_group_messages_rpc_params_for_client(
                        self.client,
                        group.as_str(),
                        page_limit(input.query.limit, 50),
                        input.query.cursor.as_ref().map(crate::ids::Cursor::as_str),
                        0,
                    )?;
                let mut raw = self
                    .transport
                    .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "group.list_messages", params)
                    .await?;
                project_group_e2ee_messages_for_group_async(self.client, &mut raw, group.as_str())
                    .await;
                let mut page =
                    page_from_raw_with_group(self.client, &raw, input.query.limit, Some(&group))?;
                persist_projection_best_effort_async(
                    self.client,
                    &page.items,
                    &DirectP5ProjectionProvenance::default(),
                )
                .await;
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
    p5_provenance: &DirectP5ProjectionProvenance,
) {
    if messages.is_empty() {
        return;
    }
    if matches!(
        persist_projection(client, messages, p5_provenance),
        Ok(outcome) if outcome.stored_messages > 0
    ) {
        client.emit_committed_local_message_projection("remote_history");
    }
}

pub(crate) fn persist_projection(
    client: &crate::core::ImClient,
    messages: &[crate::messages::Message],
    p5_provenance: &DirectP5ProjectionProvenance,
) -> crate::ImResult<
    crate::internal::local_state::inbound_resolution_backlog::RemoteMessageIngestOutcome,
> {
    #[cfg(feature = "sqlite")]
    {
        let records = remote_projection_records(client, messages, p5_provenance)?;
        let mut connection = crate::internal::local_state::open_writable(
            &client.core_inner().sdk_paths().local_state.sqlite_path,
        )?;
        let transaction = connection
            .transaction()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        let outcome =
            crate::internal::local_state::inbound_resolution_backlog::ingest_remote_messages(
                &transaction,
                &records,
                "remote_history",
            )?;
        transaction
            .commit()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        Ok(outcome)
    }
    #[cfg(not(feature = "sqlite"))]
    {
        let _ = (client, messages, p5_provenance);
        Ok(Default::default())
    }
}

pub(crate) async fn persist_projection_best_effort_async(
    client: &crate::core::ImClient,
    messages: &[crate::messages::Message],
    p5_provenance: &DirectP5ProjectionProvenance,
) {
    if messages.is_empty() {
        return;
    }
    if matches!(
        persist_projection_async(client, messages, p5_provenance).await,
        Ok(outcome) if outcome.stored_messages > 0
    ) {
        client.emit_committed_local_message_projection("remote_history");
    }
}

pub(crate) async fn persist_projection_async(
    client: &crate::core::ImClient,
    messages: &[crate::messages::Message],
    p5_provenance: &DirectP5ProjectionProvenance,
) -> crate::ImResult<
    crate::internal::local_state::inbound_resolution_backlog::RemoteMessageIngestOutcome,
> {
    #[cfg(feature = "sqlite")]
    {
        let records = remote_projection_records(client, messages, p5_provenance)?;
        let outcome = client
            .core_inner()
            .local_state_db()
            .await?
            .store_remote_messages(records, "remote_history")
            .await?;
        Ok(outcome)
    }
    #[cfg(not(feature = "sqlite"))]
    {
        let _ = (client, messages, p5_provenance);
        Ok(Default::default())
    }
}

#[cfg(feature = "sqlite")]
fn remote_projection_records(
    client: &crate::core::ImClient,
    messages: &[crate::messages::Message],
    p5_provenance: &DirectP5ProjectionProvenance,
) -> crate::ImResult<Vec<crate::internal::local_state::messages::MessageRecord>> {
    messages
        .iter()
        .map(|message| {
            let mut record =
                crate::internal::message_runtime::local_projection::message_record_from_message(
                    client, message,
                )?;
            let binding = p5_provenance.binding_for_message(message);
            if let Some(binding) = binding {
                if let Some(wire_peer_did) = p5_projection_wire_peer_did(
                    message,
                    &record,
                    binding,
                    p5_provenance.expected_peer_did.as_deref(),
                    p5_provenance.verified_scoped_route_for_message(message),
                ) {
                    record = record.with_resolved_wire_thread("direct", wire_peer_did);
                    persist_p5_cache_binding_metadata(&mut record, binding)?;
                }
            } else if record.wire_thread_kind == "thread"
                && message.group.is_none()
                && crate::internal::message_runtime::local_projection::peer_scope_from_metadata(
                    &message.metadata,
                )
                .is_some()
            {
                let receiver_did = message
                    .receiver
                    .as_ref()
                    .map(|receiver| receiver.as_str())
                    .unwrap_or_default();
                let wire_peer_did = direct_peer_did_for_message(
                    client.did().as_str(),
                    message.sender.as_str(),
                    receiver_did,
                )
                .trim();
                if wire_peer_did.starts_with("did:") && wire_peer_did != client.did().as_str() {
                    record = record.with_resolved_wire_thread("direct", wire_peer_did);
                }
            }
            Ok(record)
        })
        .collect()
}

#[cfg(feature = "sqlite")]
fn persist_p5_cache_binding_metadata(
    record: &mut crate::internal::local_state::messages::MessageRecord,
    binding: &P5CacheBinding,
) -> crate::ImResult<()> {
    let Some(encoded) = binding.digest.strip_prefix("sha256:") else {
        return Err(crate::ImError::PermissionDenied);
    };
    if URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| crate::ImError::PermissionDenied)?
        .len()
        != 32
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let mut object = serde_json::from_str::<Value>(&record.metadata)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    object.insert(
        "raw_message_id".to_owned(),
        Value::String(binding.message_id.clone()),
    );
    for (key, value) in [
        (P5_CACHE_PROFILE_KEY, binding.profile.as_str()),
        (P5_CACHE_SENDER_DID_KEY, binding.sender_did.as_str()),
        (
            P5_CACHE_SENDER_DEVICE_ID_KEY,
            binding.sender_device_id.as_str(),
        ),
        (P5_CACHE_RECIPIENT_DID_KEY, binding.recipient_did.as_str()),
        (
            P5_CACHE_RECIPIENT_DEVICE_ID_KEY,
            binding.recipient_device_id.as_str(),
        ),
        (P5_CACHE_BINDING_DIGEST_KEY, binding.digest.as_str()),
    ] {
        object.insert(key.to_owned(), Value::String(value.to_owned()));
    }
    record.metadata = Value::Object(object).to_string();
    Ok(())
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
        if let Some(mut committed) = committed_by_id.remove(message.id.as_str()) {
            preserve_remote_position_attributes(&mut committed, message);
            *message = committed;
        }
    }
    page.items.extend(committed_by_id.into_values());
    page.has_more |= sort_dedupe_and_truncate_messages(&mut page.items, requested_limit);
}

#[cfg(feature = "sqlite")]
fn preserve_remote_position_attributes(
    committed: &mut crate::messages::Message,
    remote: &crate::messages::Message,
) {
    for key in ["raw_message_id", "group_event_seq"] {
        if committed
            .metadata
            .attributes
            .iter()
            .any(|attribute| attribute.key == key)
        {
            continue;
        }
        if let Some(attribute) = remote
            .metadata
            .attributes
            .iter()
            .find(|attribute| attribute.key == key)
        {
            committed.metadata.attributes.push(attribute.clone());
        }
    }
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
        hydration_state: crate::internal::local_state::messages::MessageHydrationState::Hydrated,
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
    messages.sort_by(compare_messages_desc);
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
            .cmp(message_timestamp(left))
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

#[cfg(feature = "sqlite")]
fn consume_system_notifications<R>(
    client: &crate::core::ImClient,
    raw: &mut Value,
    directory_transport: &mut R,
) -> Vec<crate::realtime::ImEvent>
where
    R: RpcTransport,
{
    let drained = {
        let Some(messages) = raw.get_mut("messages").and_then(Value::as_array_mut) else {
            return Vec::new();
        };
        std::mem::take(messages)
    };
    let mut retained = Vec::with_capacity(drained.len());
    let mut warnings = Vec::new();
    let mut events = Vec::new();
    for message in drained {
        match crate::internal::system_notification::dispatch::dispatch_with_transport(
            client,
            &message,
            directory_transport,
        ) {
            crate::internal::system_notification::dispatch::SystemNotificationDispatchOutcome::NotSystem => {
                retained.push(message);
            }
            crate::internal::system_notification::dispatch::SystemNotificationDispatchOutcome::NeedsHydration => {
                warnings.push("system.notification.hydration_incomplete".to_owned());
            }
            crate::internal::system_notification::dispatch::SystemNotificationDispatchOutcome::Consumed { event } => {
                events.extend(event);
            }
            crate::internal::system_notification::dispatch::SystemNotificationDispatchOutcome::Rejected { warning } => {
                warnings.push(warning);
            }
        }
    }
    *raw.get_mut("messages")
        .and_then(Value::as_array_mut)
        .expect("messages array was checked above") = retained;
    append_raw_warnings(raw, warnings);
    events
}

#[cfg(feature = "sqlite")]
async fn consume_system_notifications_async<R>(
    client: &crate::core::ImClient,
    raw: &mut Value,
    directory_transport: &mut R,
) -> Vec<crate::realtime::ImEvent>
where
    R: AsyncRpcTransport,
{
    let drained = {
        let Some(messages) = raw.get_mut("messages").and_then(Value::as_array_mut) else {
            return Vec::new();
        };
        std::mem::take(messages)
    };
    let mut retained = Vec::with_capacity(drained.len());
    let mut warnings = Vec::new();
    let mut events = Vec::new();
    for message in drained {
        match crate::internal::system_notification::dispatch::dispatch_with_transport_async(
            client,
            &message,
            directory_transport,
        )
        .await
        {
            crate::internal::system_notification::dispatch::SystemNotificationDispatchOutcome::NotSystem => {
                retained.push(message);
            }
            crate::internal::system_notification::dispatch::SystemNotificationDispatchOutcome::NeedsHydration => {
                warnings.push("system.notification.hydration_incomplete".to_owned());
            }
            crate::internal::system_notification::dispatch::SystemNotificationDispatchOutcome::Consumed { event } => {
                events.extend(event);
            }
            crate::internal::system_notification::dispatch::SystemNotificationDispatchOutcome::Rejected { warning } => {
                warnings.push(warning);
            }
        }
    }
    *raw.get_mut("messages")
        .and_then(Value::as_array_mut)
        .expect("messages array was checked above") = retained;
    append_raw_warnings(raw, warnings);
    events
}

#[cfg(feature = "sqlite")]
fn append_raw_warnings(raw: &mut Value, warnings: Vec<String>) {
    if warnings.is_empty() {
        return;
    }
    let Some(object) = raw.as_object_mut() else {
        return;
    };
    let entry = object
        .entry("warnings".to_owned())
        .or_insert_with(|| Value::Array(Vec::new()));
    let Some(existing) = entry.as_array_mut() else {
        return;
    };
    existing.extend(warnings.into_iter().map(Value::String));
}

#[cfg(feature = "sqlite")]
pub(crate) struct SystemNotificationHydration {
    pub(crate) events: Vec<crate::realtime::ImEvent>,
    pub(crate) warnings: Vec<String>,
}

#[cfg(feature = "sqlite")]
pub(crate) fn hydrate_system_notifications<T, R>(
    client: &crate::core::ImClient,
    transport: &mut T,
    directory_transport: &mut R,
    limit: i64,
) -> crate::ImResult<SystemNotificationHydration>
where
    T: AuthenticatedRpcTransport,
    R: RpcTransport,
{
    let params = crate::internal::wire::inbox::build_inbox_rpc_params(
        &crate::internal::wire::common::WireIdentity {
            did: client.did().as_str().to_owned(),
        },
        crate::internal::wire::inbox::InboxWireRequest { limit, auth: None },
    );
    let mut raw = transport.authenticated_rpc(MESSAGE_RPC_ENDPOINT, "inbox.get", params)?;
    let events = consume_system_notifications(client, &mut raw, directory_transport);
    Ok(SystemNotificationHydration {
        events,
        warnings: raw_warnings(&raw),
    })
}

#[cfg(feature = "sqlite")]
pub(crate) async fn hydrate_system_notifications_async<T, R>(
    client: &crate::core::ImClient,
    transport: &mut T,
    directory_transport: &mut R,
    limit: i64,
) -> crate::ImResult<SystemNotificationHydration>
where
    T: AsyncAuthenticatedRpcTransport,
    R: AsyncRpcTransport,
{
    let params = crate::internal::wire::inbox::build_inbox_rpc_params(
        &crate::internal::wire::common::WireIdentity {
            did: client.did().as_str().to_owned(),
        },
        crate::internal::wire::inbox::InboxWireRequest { limit, auth: None },
    );
    let mut raw = transport
        .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "inbox.get", params)
        .await?;
    let events = consume_system_notifications_async(client, &mut raw, directory_transport).await;
    Ok(SystemNotificationHydration {
        events,
        warnings: raw_warnings(&raw),
    })
}

/// Reconciles a bounded sequence of exact-device P5 Inbox pages into the
/// committed local projection. Ordinary Direct messages are intentionally
/// excluded here: their only foreground reconciliation path is account Sync v2.
#[cfg(feature = "sqlite")]
pub(crate) async fn hydrate_exact_device_secure_inbox_async<T, R>(
    client: &crate::core::ImClient,
    transport: &mut T,
    directory_transport: &mut R,
    limit: u32,
) -> crate::ImResult<Vec<String>>
where
    T: AsyncAuthenticatedRpcTransport,
    R: AsyncRpcTransport,
{
    if limit == 0 || limit > 100 {
        return Err(crate::ImError::invalid_input(
            Some("limit".to_owned()),
            "secure Inbox hydration limit must be between 1 and 100",
        ));
    }
    if sync_lane_capability_enabled_async(
        client,
        crate::internal::wire::sync_v2::SyncLaneV3::P5Device,
    )
    .await?
    {
        return Ok(Vec::new());
    }
    const MAX_PAGES_PER_RUN: usize = 100;
    let identity = crate::internal::wire::common::WireIdentity {
        did: client.did().as_str().to_owned(),
    };
    let mut warnings = Vec::new();
    for _ in 0..MAX_PAGES_PER_RUN {
        let params = crate::internal::wire::inbox::build_exact_device_secure_inbox_rpc_params(
            &identity, limit,
        );
        let mut raw = transport
            .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "inbox.get", params)
            .await?;
        let has_more = raw
            .get("has_more")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        // The service selector is closed, but the client keeps an independent
        // admission boundary so a mixed or older response can never reintroduce
        // ordinary Inbox as a fallback around Sync v2.
        let selected_count = raw
            .get_mut("messages")
            .and_then(Value::as_array_mut)
            .map(|messages| {
                messages.retain(is_p5_v2_projection_candidate);
                messages.len()
            })
            .unwrap_or_default();
        let mut p5_provenance =
            project_secure_direct_messages_async(client, &mut raw, directory_transport).await;
        annotate_direct_peer_scopes_async(
            client,
            &mut raw,
            directory_transport,
            None,
            None,
            Some(&mut p5_provenance),
        )
        .await;
        let page = page_from_raw(client, &raw, crate::ids::PageLimit::new(limit)?)?;
        if !page.items.is_empty() {
            let outcome = persist_projection_async(client, &page.items, &p5_provenance).await?;
            if outcome.stored_messages > 0 {
                client.emit_committed_local_message_projection("secure_inbox_hydration");
            }
        }
        warnings.extend(raw_warnings(&raw));

        let acknowledged_ids = p5_provenance.consumed_raw_message_ids();
        if acknowledged_ids.is_empty() {
            if selected_count > 0 || has_more {
                return Err(secure_inbox_protocol_error(
                    "secure_inbox_hydration_no_progress",
                    "secure Inbox hydration could not advance the unread page",
                ));
            }
            return Ok(warnings);
        }
        // Root-import controls can advance the current device generation while
        // projecting this page. ACK must load the newly persisted bearer rather
        // than reuse the pre-promotion session held by the Inbox transport.
        transport.reload_authentication_state()?;
        let ack_params = crate::internal::wire::inbox::build_mark_read_rpc_params(
            &identity,
            crate::internal::wire::inbox::MarkReadWireRequest {
                message_ids: acknowledged_ids.clone(),
            },
        )?;
        let ack = transport
            .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "inbox.mark_read", ack_params)
            .await?;
        warnings.extend(raw_warnings(&ack));
        let expected = i64::try_from(acknowledged_ids.len()).unwrap_or(i64::MAX);
        if ack.get("updated_count").and_then(Value::as_i64) != Some(expected) {
            return Err(secure_inbox_protocol_error(
                "secure_inbox_ack_incomplete",
                "secure Inbox acknowledgement did not cover the committed page",
            ));
        }
        if !has_more {
            return Ok(warnings);
        }
    }
    Err(secure_inbox_protocol_error(
        "secure_inbox_hydration_limit_reached",
        "secure Inbox hydration did not converge within its bounded page limit",
    ))
}

#[cfg(feature = "sqlite")]
fn secure_inbox_protocol_error(code: &str, message: &str) -> crate::ImError {
    crate::ImError::Service {
        status_code: None,
        code: Some(code.to_owned()),
        message: message.to_owned(),
        data: None,
    }
}

#[cfg(feature = "sqlite")]
pub(crate) async fn hydrate_reliable_direct_message_async(
    client: &crate::core::ImClient,
    expected_message_id: &str,
    limit: i64,
) -> crate::ImResult<Vec<String>> {
    if expected_message_id.trim().is_empty() || limit <= 0 {
        return Err(crate::ImError::PermissionDenied);
    }
    if sync_lane_capability_enabled_async(
        client,
        crate::internal::wire::sync_v2::SyncLaneV3::P5Device,
    )
    .await?
    {
        return Err(crate::ImError::unsupported(
            "reliable-p5-hydration-owned-by-sync-lane",
        ));
    }
    let params = crate::internal::wire::inbox::build_inbox_rpc_params(
        &crate::internal::wire::common::WireIdentity {
            did: client.did().as_str().to_owned(),
        },
        crate::internal::wire::inbox::InboxWireRequest { limit, auth: None },
    );
    let mut transport = crate::internal::transport::CoreHttpTransport::new(client);
    let mut raw = AsyncAuthenticatedRpcTransport::authenticated_rpc(
        &mut transport,
        MESSAGE_RPC_ENDPOINT,
        "inbox.get",
        params,
    )
    .await?;
    let exact_message_present = raw
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|message| {
            if message
                .get("accepted_at")
                .and_then(Value::as_str)
                .is_none_or(|accepted_at| accepted_at.trim().is_empty())
            {
                return false;
            }
            let wire = p5_v2_wire_projection(message);
            matches!(
                crate::internal::secure_direct::v2_product::parse_v2_wire_message(&wire),
                Ok(Some((metadata, _))) if metadata.message_id == expected_message_id
            )
        });
    if !exact_message_present {
        return Err(crate::ImError::unsupported(
            "reliable-p5-hydration-message-not-found",
        ));
    }
    consume_group_e2ee_control_messages_async(client, &mut raw).await;
    let mut directory_transport = crate::internal::transport::CoreHttpTransport::new(client);
    let p5_provenance =
        project_secure_direct_messages_async(client, &mut raw, &mut directory_transport).await;
    let page = page_from_raw(
        client,
        &raw,
        crate::ids::PageLimit::new(
            u32::try_from(limit).map_err(|_| crate::ImError::PermissionDenied)?,
        )
        .map_err(|_| crate::ImError::PermissionDenied)?,
    )?;
    persist_projection_best_effort_async(client, &page.items, &p5_provenance).await;
    Ok(raw_warnings(&raw))
}

#[cfg(feature = "sqlite")]
fn raw_warnings(raw: &Value) -> Vec<String> {
    raw.get("warnings")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect()
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
                .filter(|item| {
                    let normalized =
                        crate::internal::system_notification::dispatch::normalize_delivery(item);
                    !crate::internal::system_notification::wire::is_trusted_delivery_marker(item)
                        && !crate::internal::system_notification::wire::is_system_namespace(
                            &normalized,
                        )
                })
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
        .collect::<crate::ImResult<Vec<_>>>()?
        .into_iter()
        .filter(|message| !is_direct_e2ee_wire_sdk_message(message))
        .collect();
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
    expected_peer_did: Option<&str>,
    mut p5_provenance: Option<&mut DirectP5ProjectionProvenance>,
) {
    if raw.get("messages").and_then(Value::as_array).is_none() {
        return;
    }
    let attempted_page_resolution = preferred_scope.is_none() && expected_peer_did.is_some();
    let resolved_page_scope = if preferred_scope.is_none() {
        expected_peer_did
            .and_then(|peer_did| resolve_direct_peer_scope(client, directory_transport, peer_did))
    } else {
        None
    };
    let messages = raw
        .get_mut("messages")
        .and_then(Value::as_array_mut)
        .expect("messages array was checked before page scope resolution");
    for message in messages {
        annotate_direct_peer_scope(
            client,
            message,
            directory_transport,
            preferred_scope,
            resolved_page_scope.as_ref(),
            attempted_page_resolution,
            expected_peer_did,
            p5_provenance.as_deref_mut(),
        );
    }
}

fn annotate_direct_peer_scope(
    client: &crate::core::ImClient,
    message: &mut Value,
    directory_transport: &mut impl RpcTransport,
    preferred_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
    resolved_page_scope: Option<&ResolvedDirectPeerScope>,
    attempted_page_resolution: bool,
    expected_peer_did: Option<&str>,
    p5_provenance: Option<&mut DirectP5ProjectionProvenance>,
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
    if expected_peer_did.is_some_and(|expected| expected.trim() != peer_did) {
        return;
    }
    if let Some(scope) = preferred_scope {
        annotate_object_with_peer_scope(object, scope, Some(peer_did));
        if expected_peer_did.is_some() {
            if let Some(provenance) = p5_provenance {
                provenance.record_verified_peer_scope(object, scope, peer_did);
            }
        }
        return;
    }
    if let Some(resolved) = resolved_page_scope {
        annotate_object_with_resolved_peer_scope(object, peer_did, resolved, p5_provenance);
        return;
    }
    if attempted_page_resolution {
        return;
    }
    if let Some(resolved) = resolve_direct_peer_scope(client, directory_transport, peer_did) {
        annotate_object_with_resolved_peer_scope(object, peer_did, &resolved, p5_provenance);
    }
}

pub(crate) async fn annotate_direct_peer_scopes_async(
    client: &crate::core::ImClient,
    raw: &mut Value,
    directory_transport: &mut impl AsyncRpcTransport,
    preferred_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
    expected_peer_did: Option<&str>,
    mut p5_provenance: Option<&mut DirectP5ProjectionProvenance>,
) {
    if raw.get("messages").and_then(Value::as_array).is_none() {
        return;
    }
    let attempted_page_resolution = preferred_scope.is_none() && expected_peer_did.is_some();
    let resolved_page_scope = if preferred_scope.is_none() {
        match expected_peer_did {
            Some(peer_did) => {
                resolve_direct_peer_scope_async(client, directory_transport, peer_did).await
            }
            None => None,
        }
    } else {
        None
    };
    let messages = raw
        .get_mut("messages")
        .and_then(Value::as_array_mut)
        .expect("messages array was checked before page scope resolution");
    for message in messages {
        annotate_direct_peer_scope_async(
            client,
            message,
            directory_transport,
            preferred_scope,
            resolved_page_scope.as_ref(),
            attempted_page_resolution,
            expected_peer_did,
            p5_provenance.as_deref_mut(),
        )
        .await;
    }
}

async fn annotate_direct_peer_scope_async(
    client: &crate::core::ImClient,
    message: &mut Value,
    directory_transport: &mut impl AsyncRpcTransport,
    preferred_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
    resolved_page_scope: Option<&ResolvedDirectPeerScope>,
    attempted_page_resolution: bool,
    expected_peer_did: Option<&str>,
    p5_provenance: Option<&mut DirectP5ProjectionProvenance>,
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
    if expected_peer_did.is_some_and(|expected| expected.trim() != peer_did) {
        return;
    }
    if let Some(scope) = preferred_scope {
        annotate_object_with_peer_scope(object, scope, Some(peer_did));
        if expected_peer_did.is_some() {
            if let Some(provenance) = p5_provenance {
                provenance.record_verified_peer_scope(object, scope, peer_did);
            }
        }
        return;
    }
    if let Some(resolved) = resolved_page_scope {
        annotate_object_with_resolved_peer_scope(object, peer_did, resolved, p5_provenance);
        return;
    }
    if attempted_page_resolution {
        return;
    }
    if let Some(resolved) =
        resolve_direct_peer_scope_async(client, directory_transport, peer_did).await
    {
        annotate_object_with_resolved_peer_scope(object, peer_did, &resolved, p5_provenance);
    }
}

fn annotate_object_with_resolved_peer_scope(
    object: &mut Map<String, Value>,
    peer_did: &str,
    resolved: &ResolvedDirectPeerScope,
    p5_provenance: Option<&mut DirectP5ProjectionProvenance>,
) {
    if !resolved.cache_attestable
        && p5_provenance
            .as_deref()
            .is_some_and(|provenance| provenance.has_binding_for_object(object))
    {
        return;
    }
    annotate_object_with_peer_scope(object, &resolved.scope, Some(peer_did));
    if resolved.cache_attestable {
        let Some(provenance) = p5_provenance else {
            return;
        };
        provenance.record_verified_peer_scope(object, &resolved.scope, peer_did);
    }
}

enum VerifiedHandleScopeLookup {
    Verified(crate::internal::local_state::owner_scope::DirectPeerScope),
    Unavailable,
    Rejected,
}

struct ResolvedDirectPeerScope {
    scope: crate::internal::local_state::owner_scope::DirectPeerScope,
    cache_attestable: bool,
}

impl std::ops::Deref for ResolvedDirectPeerScope {
    type Target = crate::internal::local_state::owner_scope::DirectPeerScope;

    fn deref(&self) -> &Self::Target {
        &self.scope
    }
}

fn resolve_direct_peer_scope(
    client: &crate::core::ImClient,
    directory_transport: &mut impl RpcTransport,
    peer_did: &str,
) -> Option<ResolvedDirectPeerScope> {
    match lookup_direct_peer_scope(client, directory_transport, peer_did) {
        VerifiedHandleScopeLookup::Verified(scope) => Some(ResolvedDirectPeerScope {
            scope,
            cache_attestable: true,
        }),
        VerifiedHandleScopeLookup::Rejected => None,
        VerifiedHandleScopeLookup::Unavailable => {
            let call =
                crate::internal::identity_wire::profile::build_profile_resolve_rpc_call(peer_did)
                    .ok()?;
            let raw = directory_transport
                .rpc(call.endpoint, call.method, call.params)
                .ok()?;
            direct_peer_scope_from_profile(raw, peer_did).map(|scope| ResolvedDirectPeerScope {
                scope,
                cache_attestable: false,
            })
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
        return VerifiedHandleScopeLookup::Rejected;
    };
    let raw = match directory_transport.rpc(call.endpoint, call.method, call.params) {
        Ok(raw) => raw,
        Err(error) if is_legacy_handle_lookup_error(&error) => {
            return VerifiedHandleScopeLookup::Unavailable;
        }
        Err(_) => return VerifiedHandleScopeLookup::Rejected,
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
) -> Option<ResolvedDirectPeerScope> {
    match lookup_direct_peer_scope_async(client, directory_transport, peer_did).await {
        VerifiedHandleScopeLookup::Verified(scope) => Some(ResolvedDirectPeerScope {
            scope,
            cache_attestable: true,
        }),
        VerifiedHandleScopeLookup::Rejected => None,
        VerifiedHandleScopeLookup::Unavailable => {
            let call =
                crate::internal::identity_wire::profile::build_profile_resolve_rpc_call(peer_did)
                    .ok()?;
            let raw = directory_transport
                .rpc(call.endpoint, call.method, call.params)
                .await
                .ok()?;
            direct_peer_scope_from_profile(raw, peer_did).map(|scope| ResolvedDirectPeerScope {
                scope,
                cache_attestable: false,
            })
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
        return VerifiedHandleScopeLookup::Rejected;
    };
    let raw = match directory_transport
        .rpc(call.endpoint, call.method, call.params)
        .await
    {
        Ok(raw) => raw,
        Err(error) if is_legacy_handle_lookup_error(&error) => {
            return VerifiedHandleScopeLookup::Unavailable;
        }
        Err(_) => return VerifiedHandleScopeLookup::Rejected,
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

fn is_legacy_handle_lookup_error(error: &crate::ImError) -> bool {
    match error {
        crate::ImError::PeerNotFound { .. } => true,
        crate::ImError::UnsupportedCapability { capability } => matches!(
            capability.trim().to_ascii_lowercase().as_str(),
            "handle.lookup" | "handle-lookup" | "directory.handle.lookup" | "directory-lookup"
        ),
        crate::ImError::Service {
            status_code, code, ..
        } => {
            if let Some(status_code) = status_code {
                return *status_code == 404;
            }
            matches!(
                code.as_deref()
                    .map(str::trim)
                    .map(str::to_ascii_lowercase)
                    .as_deref(),
                Some(
                    "-32601"
                        | "method_not_found"
                        | "rpc.method_not_found"
                        | "handle_not_found"
                        | "handle.not_found"
                        | "peer_not_found"
                        | "not_found"
                )
            )
        }
        _ => false,
    }
}

fn direct_peer_scope_from_profile(
    raw: Value,
    peer_did: &str,
) -> Option<crate::internal::local_state::owner_scope::DirectPeerScope> {
    if !profile_did_claims_match(&raw, peer_did) {
        return None;
    }
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

fn profile_did_claims_match(raw: &Value, peer_did: &str) -> bool {
    for pointer in [
        "/did",
        "/id",
        "/subject_did",
        "/subjectDid",
        "/profile/did",
        "/profile/id",
        "/profile/subject_did",
        "/profile/subjectDid",
        "/result/did",
        "/result/id",
        "/result/subject_did",
        "/result/subjectDid",
        "/subject/did",
        "/subject/id",
        "/subject/subject_did",
        "/subject/subjectDid",
        "/profile/subject/did",
        "/profile/subject/id",
        "/profile/subject/subject_did",
        "/profile/subject/subjectDid",
        "/result/subject/did",
        "/result/subject/id",
        "/result/subject/subject_did",
        "/result/subject/subjectDid",
        "/result/profile/did",
        "/result/profile/id",
        "/result/profile/subject_did",
        "/result/profile/subjectDid",
        "/result/profile/subject/did",
        "/result/profile/subject/id",
        "/result/profile/subject/subject_did",
        "/result/profile/subject/subjectDid",
    ] {
        let Some(value) = raw.pointer(pointer) else {
            continue;
        };
        let Some(claimed_did) = value
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return false;
        };
        if claimed_did != peer_did {
            return false;
        }
    }
    for pointer in [
        "/subject",
        "/profile/subject",
        "/result/subject",
        "/result/profile/subject",
    ] {
        let Some(claimed_did) = raw
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| value.starts_with("did:"))
        else {
            continue;
        };
        if claimed_did != peer_did {
            return false;
        }
    }
    true
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
) -> DirectP5ProjectionProvenance {
    clear_untrusted_security_markers_in_raw(raw);
    if sync_lane_capability_enabled_blocking(
        client,
        crate::internal::wire::sync_v2::SyncLaneV3::P5Device,
    ) {
        retain_non_p5_messages(raw);
        return DirectP5ProjectionProvenance::default();
    }
    project_secure_direct_messages_impl(client, raw, directory_transport, true, None)
}

pub(crate) fn project_secure_direct_messages_for_peer(
    client: &crate::core::ImClient,
    raw: &mut Value,
    directory_transport: &mut impl RpcTransport,
    expected_peer_did: &str,
) -> DirectP5ProjectionProvenance {
    project_secure_direct_messages_impl(
        client,
        raw,
        directory_transport,
        true,
        Some(expected_peer_did),
    )
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
    expected_peer_did: Option<&str>,
) -> DirectP5ProjectionProvenance {
    clear_untrusted_security_markers_in_raw(raw);
    #[cfg(not(feature = "sqlite"))]
    {
        let _ = (
            client,
            raw,
            directory_transport,
            redact_attachment_secrets,
            expected_peer_did,
        );
        DirectP5ProjectionProvenance::default()
    }
    #[cfg(feature = "sqlite")]
    {
        let Some(messages) = raw.get_mut("messages").and_then(Value::as_array_mut) else {
            return DirectP5ProjectionProvenance::default();
        };
        let mut message_values = std::mem::take(messages);
        // Legacy private-delivery rows are never projected. Standard P5
        // Init/Cipher traffic is handled by the ordinary P5 product path.
        message_values.retain(|message| message.get("private_transport_context").is_none());
        clear_untrusted_p5_projection_state(&mut message_values);
        let cached_secure_indices =
            apply_cached_secure_direct_messages(client, &mut message_values);
        let mut p5_provenance =
            p5_projection_provenance_for_applied_indices(&message_values, &cached_secure_indices);
        message_values = message_values
            .into_iter()
            .enumerate()
            .filter_map(|(index, message)| {
                (!is_p5_v2_projection_candidate(&message) || cached_secure_indices.contains(&index))
                    .then_some(message)
            })
            .collect();
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
        if let Some(expected_peer_did) = expected_peer_did {
            retain_direct_message_values_for_expected_peer(
                client,
                &mut filtered,
                expected_peer_did,
            );
        }
        cache_attachment_manifests_for_internal_download(client, &filtered);
        if redact_attachment_secrets {
            redact_attachment_manifests_for_public_projection(&mut filtered);
        }
        p5_provenance.retain_unambiguous_projected_instances(client, &filtered);
        *messages = filtered;
        append_secure_direct_warnings(raw, warnings);
        p5_provenance
    }
}

#[cfg(feature = "sqlite")]
pub(crate) fn project_secure_direct_messages_for_attachment_download(
    client: &crate::core::ImClient,
    raw: &mut Value,
    directory_transport: &mut impl RpcTransport,
    expected_peer_did: &str,
) -> DirectP5ProjectionProvenance {
    project_secure_direct_messages_impl(
        client,
        raw,
        directory_transport,
        false,
        Some(expected_peer_did),
    )
}

pub(crate) async fn project_secure_direct_messages_async(
    client: &crate::core::ImClient,
    raw: &mut Value,
    directory_transport: &mut impl AsyncRpcTransport,
) -> DirectP5ProjectionProvenance {
    clear_untrusted_security_markers_in_raw(raw);
    if sync_lane_capability_enabled_async(
        client,
        crate::internal::wire::sync_v2::SyncLaneV3::P5Device,
    )
    .await
    .unwrap_or(true)
    {
        retain_non_p5_messages(raw);
        return DirectP5ProjectionProvenance::default();
    }
    project_secure_direct_messages_async_impl(
        client,
        raw,
        directory_transport,
        true,
        None,
        Some(
            crate::internal::identity_root_import_completion::TrustedDirectDeliverySource::Mailbox,
        ),
    )
    .await
}

pub(crate) async fn project_secure_direct_messages_from_reliable_sync_async(
    client: &crate::core::ImClient,
    raw: &mut Value,
    directory_transport: &mut impl AsyncRpcTransport,
) -> DirectP5ProjectionProvenance {
    project_secure_direct_messages_async_impl(
        client,
        raw,
        directory_transport,
        true,
        None,
        Some(
            crate::internal::identity_root_import_completion::TrustedDirectDeliverySource::ReliableSync,
        ),
    )
    .await
}

pub(crate) async fn project_secure_direct_messages_async_for_peer(
    client: &crate::core::ImClient,
    raw: &mut Value,
    directory_transport: &mut impl AsyncRpcTransport,
    expected_peer_did: &str,
) -> DirectP5ProjectionProvenance {
    project_secure_direct_messages_async_impl(
        client,
        raw,
        directory_transport,
        true,
        Some(expected_peer_did),
        None,
    )
    .await
}

async fn project_secure_direct_messages_async_impl(
    client: &crate::core::ImClient,
    raw: &mut Value,
    directory_transport: &mut impl AsyncRpcTransport,
    redact_attachment_secrets: bool,
    expected_peer_did: Option<&str>,
    trusted_delivery_source: Option<
        crate::internal::identity_root_import_completion::TrustedDirectDeliverySource,
    >,
) -> DirectP5ProjectionProvenance {
    clear_untrusted_security_markers_in_raw(raw);
    #[cfg(not(feature = "sqlite"))]
    {
        let _ = (
            client,
            raw,
            directory_transport,
            redact_attachment_secrets,
            expected_peer_did,
            trusted_delivery_source,
        );
        DirectP5ProjectionProvenance::default()
    }
    #[cfg(feature = "sqlite")]
    {
        let Some(messages) = raw.get_mut("messages").and_then(Value::as_array_mut) else {
            return DirectP5ProjectionProvenance::default();
        };
        let mut message_values = std::mem::take(messages);
        clear_untrusted_p5_projection_state(&mut message_values);
        let cached_secure_indices =
            apply_cached_secure_direct_messages_async(client, &mut message_values).await;
        let mut p5_provenance =
            p5_projection_provenance_for_applied_indices(&message_values, &cached_secure_indices);
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
                // Forward migrations remove legacy private-delivery rows.
                // Fail closed while an old row is still observable.
                mark_suppressed_secure_control(&mut message_values[index]);
                processed_async[index] = true;
                continue;
            }
            if is_p5_v2_projection_candidate(&message_values[index]) {
                processed_async[index] = true;
                if cached_secure_indices.contains(&index) {
                    continue;
                }
                if !client.core_inner().direct_e2ee_v2_enabled() {
                    mark_suppressed_secure_control(&mut message_values[index]);
                    continue;
                }
                let wire_message = p5_v2_wire_projection(&message_values[index]);
                let (metadata, body) =
                    match crate::internal::secure_direct::v2_product::parse_v2_wire_message(
                        &wire_message,
                    ) {
                        Ok(Some(value)) => value,
                        Ok(None) | Err(_) => {
                            mark_suppressed_secure_control(&mut message_values[index]);
                            continue;
                        }
                    };
                let cache_binding = match p5_cache_binding(&metadata, &body) {
                    Ok(binding) => binding,
                    Err(_) => {
                        mark_suppressed_secure_control(&mut message_values[index]);
                        continue;
                    }
                };
                let accepted_at = message_values[index]
                    .get("accepted_at")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
                let delivery = trusted_delivery_source
                    .and_then(|source| {
                        crate::internal::identity_root_import_completion::TrustedDirectDeliveryContext::from_stored_message(
                            &metadata,
                            accepted_at,
                            source,
                        )
                        .ok()
                    });
                let core = client.core_handle();
                match crate::internal::secure_direct::v2_product::receive_for_client_scoped(
                    &core,
                    client,
                    true,
                    metadata,
                    body,
                    expected_peer_did,
                    delivery.as_ref(),
                )
                .await
                {
                    Ok(outcome) => {
                        let logical_message_id = match &outcome {
                            crate::internal::secure_direct::v2_product::V2InboundProductOutcome::Business(projection) => {
                                Some(projection.logical_message_id.clone())
                            }
                            crate::internal::secure_direct::v2_product::V2InboundProductOutcome::OwnSync(projection) => {
                                Some(projection.logical_message_id.clone())
                            }
                            _ => None,
                        };
                        apply_p5_v2_product_outcome(&mut message_values[index], outcome.clone());
                        if let Some(logical_message_id) = logical_message_id {
                            p5_provenance.record(&logical_message_id, cache_binding);
                        } else {
                            match outcome {
                                crate::internal::secure_direct::v2_product::V2InboundProductOutcome::Replay => {
                                    p5_provenance.record_replay(&cache_binding.message_id);
                                }
                                crate::internal::secure_direct::v2_product::V2InboundProductOutcome::ConsumedControl
                                | crate::internal::secure_direct::v2_product::V2InboundProductOutcome::SuppressedControl => {
                                    p5_provenance.record_terminal_control(&cache_binding.message_id);
                                }
                                crate::internal::secure_direct::v2_product::V2InboundProductOutcome::Business(_)
                                | crate::internal::secure_direct::v2_product::V2InboundProductOutcome::OwnSync(_) => {
                                    unreachable!("business outcomes carry a logical message id")
                                }
                            }
                        }
                    }
                    Err(_) => {
                        // Scoped P5 validation runs inside the ratchet transaction,
                        // before replay state commits. A rejected delivery is
                        // item-local so another valid delivery on this page can
                        // still be projected and persisted.
                        mark_suppressed_secure_control(&mut message_values[index]);
                    }
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
        if let Some(expected_peer_did) = expected_peer_did {
            retain_direct_message_values_for_expected_peer(
                client,
                &mut filtered,
                expected_peer_did,
            );
        }
        cache_attachment_manifests_for_internal_download_async(client, &filtered).await;
        if redact_attachment_secrets {
            redact_attachment_manifests_for_public_projection(&mut filtered);
        }
        p5_provenance.retain_unambiguous_projected_instances(client, &filtered);
        *messages = filtered;
        append_secure_direct_warnings(raw, compact_secure_direct_warnings(async_warnings));
        p5_provenance
    }
}

#[cfg(feature = "sqlite")]
pub(crate) async fn project_secure_direct_messages_for_attachment_download_async(
    client: &crate::core::ImClient,
    raw: &mut Value,
    directory_transport: &mut impl AsyncRpcTransport,
    expected_peer_did: &str,
) -> DirectP5ProjectionProvenance {
    project_secure_direct_messages_async_impl(
        client,
        raw,
        directory_transport,
        false,
        Some(expected_peer_did),
        None,
    )
    .await
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

fn is_p5_v2_projection_candidate(message: &Value) -> bool {
    message
        .get("meta")
        .or_else(|| message.pointer("/params/meta"))
        .and_then(|meta| meta.get("profile"))
        .and_then(Value::as_str)
        == Some(anp::direct_e2ee::DIRECT_E2EE_PROFILE_V2)
}

fn retain_non_p5_messages(raw: &mut Value) {
    if let Some(messages) = raw.get_mut("messages").and_then(Value::as_array_mut) {
        messages.retain(|message| !is_p5_v2_projection_candidate(message));
    }
}

#[cfg(feature = "sqlite")]
async fn sync_lane_capability_enabled_async(
    client: &crate::core::ImClient,
    lane: crate::internal::wire::sync_v2::SyncLaneV3,
) -> crate::ImResult<bool> {
    let db = client.core_inner().local_state_db().await?;
    let owner_identity_id = client.current_identity().id.as_str().to_owned();
    let Some(binding) = db
        .load_identity_account_binding(owner_identity_id.clone())
        .await?
    else {
        return Ok(false);
    };
    if db
        .lane_capability_negotiation_required(
            owner_identity_id.clone(),
            binding.device_auth_generation,
        )
        .await?
    {
        // An existing v2 installation must bootstrap the lane capability set
        // before selecting either transport. Do not race legacy Inbox against
        // the first lane-enabled sync.delta in the same foreground run.
        return Ok(true);
    }
    Ok(db
        .load_lane_sync_states(owner_identity_id)
        .await?
        .into_iter()
        .any(|state| state.lane == lane))
}

#[cfg(not(feature = "sqlite"))]
async fn sync_lane_capability_enabled_async(
    _client: &crate::core::ImClient,
    _lane: crate::internal::wire::sync_v2::SyncLaneV3,
) -> crate::ImResult<bool> {
    Ok(false)
}

#[cfg(feature = "sqlite")]
fn sync_lane_capability_enabled_blocking(
    client: &crate::core::ImClient,
    lane: crate::internal::wire::sync_v2::SyncLaneV3,
) -> bool {
    let connection = match crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    ) {
        Ok(connection) => connection,
        Err(_) => return true,
    };
    let owner_identity_id = client.current_identity().id.as_str();
    let binding = match crate::internal::local_state::sync_v2::load_identity_account_binding(
        &connection,
        owner_identity_id,
    ) {
        Ok(Some(binding)) => binding,
        Ok(None) | Err(crate::ImError::IdentityBindingConflict { .. }) => return false,
        Err(_) => return true,
    };
    match crate::internal::local_state::sync_v2::lane_capability_negotiation_required(
        &connection,
        owner_identity_id,
        &binding.device_auth_generation,
    ) {
        Ok(true) => true,
        Err(_) => true,
        Ok(false) => match crate::internal::local_state::sync_v2::load_lane_sync_states(
            &connection,
            owner_identity_id,
        ) {
            Ok(states) => states.into_iter().any(|state| state.lane == lane),
            Err(crate::ImError::IdentityBindingConflict { .. }) => false,
            Err(_) => true,
        },
    }
}

#[cfg(not(feature = "sqlite"))]
fn sync_lane_capability_enabled_blocking(
    _client: &crate::core::ImClient,
    _lane: crate::internal::wire::sync_v2::SyncLaneV3,
) -> bool {
    false
}

fn is_p6_v2_projection_candidate(message: &Value) -> bool {
    message
        .get("meta")
        .or_else(|| message.pointer("/params/meta"))
        .and_then(|meta| meta.get("profile"))
        .and_then(Value::as_str)
        == Some(anp::group_e2ee::GROUP_E2EE_PROFILE_V2)
}

#[cfg(feature = "group-e2ee")]
fn local_group_requires_p6(client: &crate::core::ImClient, group_did: &str) -> bool {
    let cached_policy_requires_p6 =
        crate::internal::group_runtime::cache::cached_group_snapshot(client, group_did)
            .ok()
            .flatten()
            .as_ref()
            .is_some_and(crate::internal::group_runtime::cache::group_snapshot_uses_e2ee);
    cached_policy_requires_p6 || local_group_has_p6_state(client, group_did)
}

#[cfg(feature = "group-e2ee")]
async fn local_group_requires_p6_async(client: &crate::core::ImClient, group_did: &str) -> bool {
    let cached_policy_requires_p6 =
        crate::internal::group_runtime::cache::cached_group_snapshot_async(client, group_did)
            .await
            .ok()
            .flatten()
            .as_ref()
            .is_some_and(crate::internal::group_runtime::cache::group_snapshot_uses_e2ee);
    cached_policy_requires_p6 || local_group_has_p6_state(client, group_did)
}

#[cfg(feature = "group-e2ee")]
fn local_group_has_p6_state(client: &crate::core::ImClient, group_did: &str) -> bool {
    use anp::group_e2ee::operations::v2::{V2InspectLocalGroupInput, V2LocalGroupReadiness};

    let Ok(runtime) = crate::internal::group_e2ee::v2_runtime::runtime_for_client(client) else {
        return false;
    };
    let Ok(scope) = runtime.owner_scope() else {
        return false;
    };
    runtime
        .inspect_local_group(V2InspectLocalGroupInput {
            owner_did: scope.owner_did,
            owner_device_id: scope.device_id,
            group_did: group_did.to_owned(),
            request_id: "p6-history-security-posture".to_owned(),
        })
        .is_ok_and(|status| status.readiness != V2LocalGroupReadiness::Missing)
}

#[cfg(feature = "group-e2ee")]
fn secure_group_remote_message_allowed(message: &Value, expected_group_did: &str) -> bool {
    if is_p6_v2_projection_candidate(message) {
        return parse_p6_v2_incoming_notification(message)
            .ok()
            .is_some_and(|(_, body, _)| body.group_did == expected_group_did);
    }
    message.get("group_did").and_then(Value::as_str) == Some(expected_group_did)
        && message.get("type").and_then(Value::as_str) == Some("system")
        && message
            .get("system_event")
            .and_then(Value::as_object)
            .is_some_and(|event| {
                event
                    .get("event_kind")
                    .and_then(Value::as_str)
                    .is_some_and(|value| !value.is_empty())
                    && event
                        .get("subject_method")
                        .and_then(Value::as_str)
                        .is_some_and(|value| {
                            !value.is_empty() && !matches!(value, "group.send" | "group.e2ee.send")
                        })
            })
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

const P5_CACHE_PROFILE_KEY: &str = "p5_cache_profile";
const P5_CACHE_SENDER_DID_KEY: &str = "p5_cache_sender_did";
const P5_CACHE_SENDER_DEVICE_ID_KEY: &str = "p5_cache_sender_device_id";
const P5_CACHE_RECIPIENT_DID_KEY: &str = "p5_cache_recipient_did";
const P5_CACHE_RECIPIENT_DEVICE_ID_KEY: &str = "p5_cache_recipient_device_id";
const P5_CACHE_BINDING_DIGEST_KEY: &str = "p5_cache_binding_digest";

const P5_CACHE_METADATA_KEYS: [&str; 6] = [
    P5_CACHE_PROFILE_KEY,
    P5_CACHE_SENDER_DID_KEY,
    P5_CACHE_SENDER_DEVICE_ID_KEY,
    P5_CACHE_RECIPIENT_DID_KEY,
    P5_CACHE_RECIPIENT_DEVICE_ID_KEY,
    P5_CACHE_BINDING_DIGEST_KEY,
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct P5CacheBinding {
    message_id: String,
    profile: String,
    sender_did: String,
    sender_device_id: String,
    recipient_did: String,
    recipient_device_id: String,
    digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct P5ProjectionInstanceKey {
    logical_message_id: String,
    raw_message_id: String,
}

impl P5ProjectionInstanceKey {
    fn from_direct_message(message: &crate::messages::Message) -> Option<Self> {
        if message.group.is_some()
            || !matches!(message.thread, crate::messages::ThreadRef::Direct(_))
        {
            return None;
        }
        Self::from_ungrouped_message(message)
    }

    fn from_ungrouped_message(message: &crate::messages::Message) -> Option<Self> {
        if message.group.is_some() {
            return None;
        }
        Some(Self {
            logical_message_id: message.id.as_str().to_owned(),
            raw_message_id: metadata_attribute(&message.metadata, "raw_message_id")
                .unwrap_or(message.id.as_str())
                .to_owned(),
        })
    }

    fn from_ungrouped_object(object: &Map<String, Value>) -> Option<Self> {
        if !string_value(object.get("group_did")).trim().is_empty() {
            return None;
        }
        let logical_message_id = string_value(object.get("id"));
        let raw_message_id = string_value(object.get("raw_message_id"));
        if logical_message_id.trim().is_empty() || raw_message_id.trim().is_empty() {
            return None;
        }
        Some(Self {
            logical_message_id,
            raw_message_id,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct P5VerifiedScopedRoute {
    peer_did: String,
    thread_id: String,
}

/// Ephemeral proof that a P5 projection in this exact Direct read call came
/// from either a successful product receive or a validated local cache hit.
///
/// It is deliberately kept outside the service response and public message
/// model so service-supplied projection flags cannot mint cache metadata.
#[derive(Debug, Default)]
pub(crate) struct DirectP5ProjectionProvenance {
    bindings: HashMap<P5ProjectionInstanceKey, Vec<P5CacheBinding>>,
    verified_scoped_routes: HashMap<P5ProjectionInstanceKey, P5VerifiedScopedRoute>,
    consumed_raw_message_ids: Vec<String>,
    consumed_raw_message_id_set: HashSet<String>,
    projected_raw_message_ids: HashSet<String>,
    terminal_control_raw_message_ids: HashSet<String>,
    replay_raw_message_ids: HashSet<String>,
    expected_peer_did: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DirectP5ProjectionDisposition {
    Projected,
    TerminalControl,
    Replay,
    NotConsumed,
}

impl DirectP5ProjectionProvenance {
    fn record(&mut self, logical_message_id: &str, binding: P5CacheBinding) {
        let logical_message_id = logical_message_id.trim();
        let raw_message_id = binding.message_id.trim();
        if logical_message_id.is_empty() || raw_message_id.is_empty() {
            return;
        }
        let key = P5ProjectionInstanceKey {
            logical_message_id: logical_message_id.to_owned(),
            raw_message_id: raw_message_id.to_owned(),
        };
        self.record_consumed_raw_message_id(raw_message_id);
        self.projected_raw_message_ids
            .insert(raw_message_id.to_owned());
        self.bindings.entry(key).or_default().push(binding);
    }

    fn record_terminal_control(&mut self, raw_message_id: &str) {
        let raw_message_id = raw_message_id.trim();
        self.record_consumed_raw_message_id(raw_message_id);
        self.terminal_control_raw_message_ids
            .insert(raw_message_id.to_owned());
    }

    fn record_replay(&mut self, raw_message_id: &str) {
        let raw_message_id = raw_message_id.trim();
        self.record_consumed_raw_message_id(raw_message_id);
        self.replay_raw_message_ids
            .insert(raw_message_id.to_owned());
    }

    fn record_consumed_raw_message_id(&mut self, raw_message_id: &str) {
        let raw_message_id = raw_message_id.trim();
        if raw_message_id.is_empty()
            || !self
                .consumed_raw_message_id_set
                .insert(raw_message_id.to_owned())
        {
            return;
        }
        self.consumed_raw_message_ids
            .push(raw_message_id.to_owned());
    }

    pub(crate) fn consumed_raw_message_ids(&self) -> Vec<String> {
        self.consumed_raw_message_ids.clone()
    }

    pub(crate) fn disposition_for_raw_message_id(
        &self,
        raw_message_id: &str,
    ) -> DirectP5ProjectionDisposition {
        if self.projected_raw_message_ids.contains(raw_message_id) {
            DirectP5ProjectionDisposition::Projected
        } else if self
            .terminal_control_raw_message_ids
            .contains(raw_message_id)
        {
            DirectP5ProjectionDisposition::TerminalControl
        } else if self.replay_raw_message_ids.contains(raw_message_id) {
            DirectP5ProjectionDisposition::Replay
        } else {
            DirectP5ProjectionDisposition::NotConsumed
        }
    }

    fn retain_unambiguous_projected_instances(
        &mut self,
        client: &crate::core::ImClient,
        messages: &[Value],
    ) {
        // Freeze ambiguity against the complete displayable projection before
        // callers dedupe inboxes or filter/truncate incremental sync pages.
        let mut logical_counts = HashMap::<String, usize>::new();
        let mut raw_counts = HashMap::<String, usize>::new();
        let mut projected_instances = HashSet::new();
        for value in messages {
            let Ok(Some(message)) = message_from_value(client, value, None) else {
                continue;
            };
            // At this point verified peer-scope metadata has not been added.
            // Only an authenticated product/cache result that still resolves
            // to the concrete Direct endpoint may retain provenance.
            let Some(key) = P5ProjectionInstanceKey::from_direct_message(&message) else {
                continue;
            };
            *logical_counts
                .entry(key.logical_message_id.clone())
                .or_default() += 1;
            *raw_counts.entry(key.raw_message_id.clone()).or_default() += 1;
            projected_instances.insert(key);
        }
        self.bindings.retain(|key, bindings| {
            bindings.len() == 1
                && projected_instances.contains(key)
                && logical_counts.get(&key.logical_message_id) == Some(&1)
                && raw_counts.get(&key.raw_message_id) == Some(&1)
        });
    }

    fn binding_for_message(&self, message: &crate::messages::Message) -> Option<&P5CacheBinding> {
        // Verified Handle projection may replace Direct(peer DID) with the
        // stable dm:peer-scope thread after provenance was frozen. Route and
        // endpoint binding are checked separately before persistence.
        let key = P5ProjectionInstanceKey::from_ungrouped_message(message)?;
        let bindings = self.bindings.get(&key)?;
        if bindings.len() != 1 {
            return None;
        }
        bindings.first()
    }

    fn record_verified_peer_scope(
        &mut self,
        object: &Map<String, Value>,
        scope: &crate::internal::local_state::owner_scope::DirectPeerScope,
        peer_did: &str,
    ) {
        let Some(key) = P5ProjectionInstanceKey::from_ungrouped_object(object) else {
            return;
        };
        let Some(bindings) = self.bindings.get(&key) else {
            return;
        };
        let Some(binding) = (bindings.len() == 1).then(|| &bindings[0]) else {
            return;
        };
        if !p5_projected_object_endpoint_matches_binding(object, peer_did, binding) {
            return;
        }
        self.verified_scoped_routes.insert(
            key,
            P5VerifiedScopedRoute {
                peer_did: peer_did.to_owned(),
                thread_id: crate::internal::message_runtime::local_projection::direct_conversation_id_for_peer_scope(scope),
            },
        );
    }

    fn verified_scoped_route_for_message(
        &self,
        message: &crate::messages::Message,
    ) -> Option<&P5VerifiedScopedRoute> {
        let key = P5ProjectionInstanceKey::from_ungrouped_message(message)?;
        self.verified_scoped_routes.get(&key)
    }

    fn has_binding_for_object(&self, object: &Map<String, Value>) -> bool {
        let Some(key) = P5ProjectionInstanceKey::from_ungrouped_object(object) else {
            return false;
        };
        self.bindings
            .get(&key)
            .is_some_and(|bindings| bindings.len() == 1)
    }
}

fn p5_projected_object_endpoint_matches_binding(
    object: &Map<String, Value>,
    peer_did: &str,
    binding: &P5CacheBinding,
) -> bool {
    let sender_did = string_value(object.get("sender_did"));
    let receiver_did = string_value(object.get("receiver_did"));
    let direction = i64_value(object.get("direction"));
    if binding.sender_did != binding.recipient_did {
        return direction == Some(0)
            && sender_did == binding.sender_did
            && receiver_did == binding.recipient_did
            && peer_did == binding.sender_did;
    }
    direction == Some(1)
        && sender_did == binding.sender_did
        && !receiver_did.trim().is_empty()
        && receiver_did != binding.recipient_did
        && peer_did == receiver_did
}

#[cfg(feature = "sqlite")]
fn p5_projection_provenance_for_applied_indices(
    messages: &[Value],
    applied: &HashSet<usize>,
) -> DirectP5ProjectionProvenance {
    let mut provenance = DirectP5ProjectionProvenance::default();
    for index in applied {
        let Some(message) = messages.get(*index) else {
            continue;
        };
        let logical_message_id = message
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let binding = p5_cache_binding_from_message(message).ok().flatten();
        let raw_message_id = message
            .get("raw_message_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let Some((logical_message_id, binding)) = logical_message_id.zip(binding) else {
            continue;
        };
        if raw_message_id == Some(binding.message_id.as_str()) {
            provenance.record(logical_message_id, binding);
        }
    }
    provenance
}

fn p5_cache_binding_from_message(message: &Value) -> crate::ImResult<Option<P5CacheBinding>> {
    if !is_p5_v2_projection_candidate(message) {
        return Ok(None);
    }
    let wire = p5_v2_wire_projection(message);
    let (metadata, body) =
        crate::internal::secure_direct::v2_product::parse_v2_wire_message(&wire)?
            .ok_or(crate::ImError::PermissionDenied)?;
    Ok(Some(p5_cache_binding(&metadata, &body)?))
}

fn p5_cache_metadata_from_message(
    message: &Value,
) -> crate::ImResult<Option<anp::direct_e2ee::V2DirectMetadata>> {
    if !is_p5_v2_projection_candidate(message) {
        return Ok(None);
    }
    let wire = p5_v2_wire_projection(message);
    let (metadata, _) = crate::internal::secure_direct::v2_product::parse_v2_wire_message(&wire)?
        .ok_or(crate::ImError::PermissionDenied)?;
    Ok(Some(metadata))
}

fn p5_cache_binding(
    metadata: &anp::direct_e2ee::V2DirectMetadata,
    body: &anp::direct_e2ee::V2DirectBody,
) -> crate::ImResult<P5CacheBinding> {
    let aad = match body {
        anp::direct_e2ee::V2DirectBody::Init(body) => {
            anp::direct_e2ee::build_init_aad_v2(metadata, body)
        }
        anp::direct_e2ee::V2DirectBody::Cipher(body) => {
            anp::direct_e2ee::build_message_aad_v2(metadata, body)
        }
    }
    .map_err(|_| crate::ImError::PermissionDenied)?;
    let body_value = match body {
        anp::direct_e2ee::V2DirectBody::Init(body) => serde_json::to_value(body),
        anp::direct_e2ee::V2DirectBody::Cipher(body) => serde_json::to_value(body),
    }
    .map_err(|error| crate::ImError::Serialization {
        detail: format!("serialize P5 cache body: {error}"),
    })?;
    let canonical_body = Zeroizing::new(serde_json_canonicalizer::to_vec(&body_value).map_err(
        |error| crate::ImError::Serialization {
            detail: format!("canonicalize P5 cache body: {error}"),
        },
    )?);
    let aad = Zeroizing::new(aad);
    let mut digest = Sha256::new();
    digest.update(b"AWIKI-P5-V2-CACHE-BINDING-V1\0");
    update_digest_part(&mut digest, &aad);
    update_digest_part(&mut digest, &canonical_body);
    Ok(P5CacheBinding {
        message_id: metadata.message_id.clone(),
        profile: metadata.profile.clone(),
        sender_did: metadata.sender_did.clone(),
        sender_device_id: metadata.sender_device_id.clone(),
        recipient_did: metadata.target.did.clone(),
        recipient_device_id: metadata.recipient_device_id.clone(),
        digest: format!("sha256:{}", URL_SAFE_NO_PAD.encode(digest.finalize())),
    })
}

fn update_digest_part(digest: &mut Sha256, value: &[u8]) {
    digest.update((value.len() as u64).to_be_bytes());
    digest.update(value);
}

#[cfg(feature = "sqlite")]
fn clear_untrusted_p5_projection_state(messages: &mut [Value]) {
    for message in messages {
        if !is_p5_v2_projection_candidate(message) {
            continue;
        }
        if let Some(object) = message.as_object_mut() {
            // These fields are local projection output. A service-provided
            // value must not be mistaken for a completed authenticated
            // receive or an accepted local cache hit.
            for key in [
                "secure",
                "decryption_state",
                "raw_message_id",
                "peer_user_id",
                "peer_full_handle",
                "peer_current_did",
                "resolved_target_did",
                "target_handle",
            ] {
                object.remove(key);
            }
        }
    }
}

fn clear_untrusted_remote_security_markers(messages: &mut [Value]) {
    for message in messages {
        let Some(object) = message.as_object_mut() else {
            continue;
        };
        // These fields are produced only after local cache authorization or
        // authenticated decryption. Remote services must never self-assert
        // them, even when they omit the P5/P6 profile discriminator.
        for key in [
            "secure",
            "decrypted",
            "decryption_state",
            "secure_wire_content_type",
        ] {
            object.remove(key);
        }
    }
}

fn clear_untrusted_security_markers_in_raw(raw: &mut Value) {
    let Some(messages) = raw.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };
    clear_untrusted_remote_security_markers(messages);
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
                object.insert(
                    "receiver_did".to_owned(),
                    Value::String(projection.recipient_did),
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
        | V2InboundProductOutcome::SuppressedControl => mark_suppressed_secure_control(message),
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
fn mark_suppressed_secure_control(message: &mut Value) {
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
        .filter_map(|message| {
            if is_p5_v2_projection_candidate(message) {
                return p5_cache_binding_from_message(message)
                    .ok()
                    .flatten()
                    .map(|binding| binding.message_id);
            }
            Some(secure_direct_message_id(message))
        })
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
    let authorized_p5_indices = authorized_cached_p5_indices(client, messages);
    apply_cached_secure_direct_records(messages, records, Some(&authorized_p5_indices))
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
    let authorized_p5_indices = authorized_cached_p5_indices(client, messages);
    apply_cached_secure_direct_records(messages, records, Some(&authorized_p5_indices))
}

#[cfg(feature = "sqlite")]
fn authorized_cached_p5_indices(
    client: &crate::core::ImClient,
    messages: &[Value],
) -> HashSet<usize> {
    if !client.core_inner().direct_e2ee_v2_enabled() {
        return HashSet::new();
    }
    messages
        .iter()
        .enumerate()
        .filter_map(|(index, message)| {
            let metadata = p5_cache_metadata_from_message(message).ok().flatten()?;
            crate::internal::secure_direct::v2_product::validate_cached_inbound_endpoint_for_client(
                &client.core_handle(),
                client,
                &metadata,
            )
            .ok()?;
            Some(index)
        })
        .collect()
}

#[cfg(feature = "sqlite")]
fn apply_cached_secure_direct_records(
    messages: &mut [Value],
    records: Vec<crate::internal::local_state::messages::MessageRecord>,
    authorized_p5_indices: Option<&HashSet<usize>>,
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
        let is_p5 = is_p5_v2_projection_candidate(message);
        if is_p5 && !authorized_p5_indices.is_some_and(|indices| indices.contains(&index)) {
            continue;
        }
        let incoming_p5_binding = p5_cache_binding_from_message(message).ok().flatten();
        let wire_message_id = incoming_p5_binding
            .as_ref()
            .map(|binding| binding.message_id.clone())
            .unwrap_or_else(|| secure_direct_message_id(message));
        let Some(candidates) = records_by_wire_id.get(&wire_message_id) else {
            continue;
        };
        let Some(record) = (candidates.len() == 1).then(|| &candidates[0]) else {
            continue;
        };
        let stored_p5_binding = p5_cache_binding_from_record(record);
        let endpoint_matches = if let (Some(incoming), Some(stored)) =
            (incoming_p5_binding.as_ref(), stored_p5_binding.as_ref())
        {
            incoming == stored
                && p5_cache_record_has_direct_route(record)
                && p5_cache_record_endpoint_matches(record, incoming)
        } else if !is_p5 && stored_p5_binding.is_none() {
            let sender_did = message
                .get("sender_did")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let receiver_did = message
                .get("receiver_did")
                .or_else(|| message.get("recipient_did"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            record.sender_did == sender_did && record.receiver_did == receiver_did
        } else {
            false
        };
        if !endpoint_matches {
            continue;
        }
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
fn p5_cache_record_has_direct_route(
    record: &crate::internal::local_state::messages::MessageRecord,
) -> bool {
    record.group_id.trim().is_empty()
        && record.group_did.trim().is_empty()
        && record.wire_thread_kind.trim() == "direct"
        && !record.wire_thread_ref.trim().is_empty()
        && record.conversation_id.trim().starts_with("dm:")
        && record.thread_id.trim().starts_with("dm:")
}

#[cfg(feature = "sqlite")]
fn p5_projection_wire_peer_did(
    message: &crate::messages::Message,
    record: &crate::internal::local_state::messages::MessageRecord,
    binding: &P5CacheBinding,
    expected_peer_did: Option<&str>,
    verified_scoped_route: Option<&P5VerifiedScopedRoute>,
) -> Option<String> {
    if !p5_cache_record_endpoint_matches(record, binding)
        || !record.group_id.trim().is_empty()
        || !record.group_did.trim().is_empty()
        || record.conversation_id.trim() != record.thread_id.trim()
        || !record.conversation_id.trim().starts_with("dm:")
    {
        return None;
    }

    let wire_peer_did = if binding.sender_did != binding.recipient_did {
        binding.sender_did.trim()
    } else {
        record.receiver_did.trim()
    };
    if !wire_peer_did.starts_with("did:") || wire_peer_did == record.owner_did.trim() {
        return None;
    }
    if expected_peer_did.is_some_and(|expected| expected.trim() != wire_peer_did) {
        return None;
    }

    let route_matches = match &message.thread {
        crate::messages::ThreadRef::Direct(peer) => peer.as_str() == wire_peer_did,
        crate::messages::ThreadRef::Thread(thread) => {
            let verified_route_matches = verified_scoped_route.is_some_and(|route| {
                route.peer_did == wire_peer_did && route.thread_id == thread.as_str()
            });
            let scope =
                crate::internal::message_runtime::local_projection::peer_scope_from_metadata(
                    &message.metadata,
                );
            let canonical_thread = scope.as_ref().map(
                crate::internal::message_runtime::local_projection::direct_conversation_id_for_peer_scope,
            );
            let record_wire_route_matches = (record.wire_thread_kind.trim() == "direct"
                && record.wire_thread_ref.trim() == wire_peer_did)
                || (record.wire_thread_kind.trim() == "thread"
                    && record.wire_thread_ref.trim() == thread.as_str());
            verified_route_matches
                && canonical_thread.as_deref() == Some(thread.as_str())
                && record.conversation_id.trim() == thread.as_str()
                && record_wire_route_matches
                && metadata_attribute(&message.metadata, "peer_current_did") == Some(wire_peer_did)
                && metadata_attribute(&message.metadata, "resolved_target_did")
                    == Some(wire_peer_did)
        }
        crate::messages::ThreadRef::Group(_) => false,
    };
    route_matches.then(|| wire_peer_did.to_owned())
}

#[cfg(feature = "sqlite")]
fn p5_cache_record_endpoint_matches(
    record: &crate::internal::local_state::messages::MessageRecord,
    binding: &P5CacheBinding,
) -> bool {
    if record.owner_did != binding.recipient_did {
        return false;
    }
    if binding.sender_did != binding.recipient_did {
        return record.direction == 0
            && record.sender_did == binding.sender_did
            && record.receiver_did == binding.recipient_did;
    }
    record.direction == 1
        && record.sender_did == record.owner_did
        && binding.sender_did == record.owner_did
        && !record.receiver_did.trim().is_empty()
        && record.receiver_did != record.owner_did
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
fn p5_cache_binding_from_record(
    record: &crate::internal::local_state::messages::MessageRecord,
) -> Option<P5CacheBinding> {
    let metadata = serde_json::from_str::<Value>(&record.metadata).ok()?;
    let string = |key: &str| {
        metadata
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };
    let digest = string(P5_CACHE_BINDING_DIGEST_KEY)?;
    let encoded = digest.strip_prefix("sha256:")?;
    if URL_SAFE_NO_PAD
        .decode(encoded)
        .ok()
        .is_none_or(|decoded| decoded.len() != 32)
    {
        return None;
    }
    let binding = P5CacheBinding {
        message_id: string("raw_message_id")?,
        profile: string(P5_CACHE_PROFILE_KEY)?,
        sender_did: string(P5_CACHE_SENDER_DID_KEY)?,
        sender_device_id: string(P5_CACHE_SENDER_DEVICE_ID_KEY)?,
        recipient_did: string(P5_CACHE_RECIPIENT_DID_KEY)?,
        recipient_device_id: string(P5_CACHE_RECIPIENT_DEVICE_ID_KEY)?,
        digest,
    };
    (binding.profile == anp::direct_e2ee::DIRECT_E2EE_PROFILE_V2).then_some(binding)
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
    project_group_e2ee_messages_impl(client, raw, true, None, false);
}

#[cfg(feature = "group-e2ee")]
pub(crate) fn project_group_e2ee_messages_for_group(
    client: &crate::core::ImClient,
    raw: &mut Value,
    group_did: &str,
) {
    project_group_e2ee_messages_impl(
        client,
        raw,
        true,
        Some(group_did),
        local_group_requires_p6(client, group_did),
    );
}

#[cfg(feature = "group-e2ee")]
fn consume_group_e2ee_control_messages(client: &crate::core::ImClient, raw: &mut Value) {
    let Some(messages) = raw.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };
    let message_values = std::mem::take(messages);
    let mut retained = Vec::with_capacity(message_values.len());
    let mut warnings = Vec::new();
    let lane_enabled = sync_lane_capability_enabled_blocking(
        client,
        crate::internal::wire::sync_v2::SyncLaneV3::P6Group,
    );
    for message in message_values {
        if crate::internal::group_e2ee::v2_notice::is_v2_notice_candidate(&message) {
            if !lane_enabled
                && client.core_inner().group_e2ee_v2_enabled()
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
    let lane_enabled = sync_lane_capability_enabled_async(
        client,
        crate::internal::wire::sync_v2::SyncLaneV3::P6Group,
    )
    .await
    .unwrap_or(true);
    for message in message_values {
        if crate::internal::group_e2ee::v2_notice::is_v2_notice_candidate(&message) {
            if !lane_enabled
                && client.core_inner().group_e2ee_v2_enabled()
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

#[cfg(feature = "group-e2ee")]
pub(crate) async fn consume_group_e2ee_control_notice_from_reliable_sync_async(
    client: &crate::core::ImClient,
    notice: &Value,
) -> crate::ImResult<()> {
    if !crate::internal::group_e2ee::v2_notice::is_v2_notice_candidate(notice)
        || !client.core_inner().group_e2ee_v2_enabled()
    {
        return Err(crate::ImError::PermissionDenied);
    }
    crate::internal::group_e2ee::v2_notice::consume_for_client_async(client, notice)
        .await
        .map(|_| ())
}

#[cfg(not(feature = "group-e2ee"))]
pub(crate) async fn consume_group_e2ee_control_notice_from_reliable_sync_async(
    _client: &crate::core::ImClient,
    _notice: &Value,
) -> crate::ImResult<()> {
    Err(crate::ImError::unsupported("group-e2ee"))
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
    expected_group_did: Option<&str>,
    requires_p6: bool,
) {
    consume_group_e2ee_control_messages(client, raw);
    let Some(messages) = raw.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };
    let mut message_values = std::mem::take(messages);
    clear_untrusted_remote_security_markers(&mut message_values);
    if let Some(group_did) = expected_group_did.filter(|_| requires_p6) {
        message_values.retain(|message| secure_group_remote_message_allowed(message, group_did));
    }
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
    let _ = apply_cached_group_e2ee_messages(client, &mut message_values);
    let warnings =
        crate::internal::group_e2ee::incoming::maybe_decrypt_group_e2ee_messages_for_client(
            client,
            &mut message_values,
        );
    cache_attachment_manifests_for_internal_download(client, &message_values);
    apply_cached_group_attachment_manifests(client, &mut message_values);
    if redact_attachment_secrets {
        redact_attachment_manifests_for_public_projection(&mut message_values);
    }
    *messages = message_values;
    append_secure_direct_warnings(raw, warnings);
}

#[cfg(not(feature = "group-e2ee"))]
pub(crate) fn project_group_e2ee_messages(_client: &crate::core::ImClient, raw: &mut Value) {
    clear_untrusted_security_markers_in_raw(raw);
}

#[cfg(not(feature = "group-e2ee"))]
pub(crate) fn project_group_e2ee_messages_for_group(
    client: &crate::core::ImClient,
    raw: &mut Value,
    _group_did: &str,
) {
    project_group_e2ee_messages(client, raw);
}

pub(crate) fn project_group_e2ee_messages_for_attachment_download(
    client: &crate::core::ImClient,
    raw: &mut Value,
) {
    #[cfg(feature = "group-e2ee")]
    project_group_e2ee_messages_impl(client, raw, false, None, false);
    #[cfg(not(feature = "group-e2ee"))]
    project_group_e2ee_messages(client, raw);
}

#[cfg(feature = "group-e2ee")]
pub(crate) async fn project_group_e2ee_messages_async(
    client: &crate::core::ImClient,
    raw: &mut Value,
) {
    project_group_e2ee_messages_async_impl(client, raw, true, None, false).await;
}

#[cfg(feature = "group-e2ee")]
pub(crate) async fn project_group_e2ee_messages_for_group_async(
    client: &crate::core::ImClient,
    raw: &mut Value,
    group_did: &str,
) {
    let requires_p6 = local_group_requires_p6_async(client, group_did).await;
    project_group_e2ee_messages_async_impl(client, raw, true, Some(group_did), requires_p6).await;
}

#[cfg(feature = "group-e2ee")]
async fn project_group_e2ee_messages_async_impl(
    client: &crate::core::ImClient,
    raw: &mut Value,
    redact_attachment_secrets: bool,
    expected_group_did: Option<&str>,
    requires_p6: bool,
) {
    consume_group_e2ee_control_messages_async(client, raw).await;
    let Some(messages) = raw.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };
    let mut message_values = std::mem::take(messages);
    clear_untrusted_remote_security_markers(&mut message_values);
    if let Some(group_did) = expected_group_did.filter(|_| requires_p6) {
        message_values.retain(|message| secure_group_remote_message_allowed(message, group_did));
    }
    clear_untrusted_p6_projection_state(&mut message_values);
    let cached_p6_indices =
        apply_cached_group_e2ee_messages_async(client, &mut message_values).await;
    let mut p6_projected = Vec::with_capacity(message_values.len());
    let mut newly_decrypted_values = Vec::new();
    let mut p6_warnings = Vec::new();
    for (index, mut message) in message_values.drain(..).enumerate() {
        if !is_p6_v2_projection_candidate(&message) {
            p6_projected.push(message);
            continue;
        }
        if !client.core_inner().group_e2ee_v2_enabled() {
            continue;
        }
        if cached_p6_indices.contains(&index) {
            strip_p6_v2_wire_fields(&mut message);
            p6_projected.push(message);
            continue;
        }
        match project_p6_v2_incoming_message(client, &mut message).await {
            Ok(()) => {
                newly_decrypted_values.push(message.clone());
                p6_projected.push(message);
            }
            Err(error) => p6_warnings.push(format!(
                "P6 v2 group message was rejected before projection ({})",
                p6_projection_error_code(&error)
            )),
        }
    }
    cache_attachment_manifests_for_internal_download_async(client, &newly_decrypted_values).await;
    let newly_decrypted = newly_decrypted_values
        .into_iter()
        .filter_map(|mut message| {
            redact_attachment_manifests_for_public_projection(std::slice::from_mut(&mut message));
            message_from_value(client, &message, None).ok().flatten()
        })
        .collect::<Vec<_>>();
    if !newly_decrypted.is_empty() {
        if persist_newly_decrypted_p6_messages_async(
            client,
            &newly_decrypted,
            "p6_history_decryption",
        )
        .await
        .is_err()
        {
            p6_warnings.push("P6 v2 group plaintext cache was not durably committed".to_owned());
        }
    }
    message_values = p6_projected;
    let _ = apply_cached_group_e2ee_messages_async(client, &mut message_values).await;
    let mut warnings =
        crate::internal::group_e2ee::incoming::maybe_decrypt_group_e2ee_messages_for_client_async(
            client,
            &mut message_values,
        )
        .await;
    warnings.extend(p6_warnings);
    cache_attachment_manifests_for_internal_download_async(client, &message_values).await;
    apply_cached_group_attachment_manifests(client, &mut message_values);
    if redact_attachment_secrets {
        redact_attachment_manifests_for_public_projection(&mut message_values);
    }
    *messages = message_values;
    append_secure_direct_warnings(raw, warnings);
}

#[cfg(feature = "group-e2ee")]
async fn persist_newly_decrypted_p6_messages_async(
    client: &crate::core::ImClient,
    messages: &[crate::messages::Message],
    source: &str,
) -> crate::ImResult<()> {
    if messages.is_empty() {
        return Ok(());
    }
    let mut retry = 0_u32;
    let outcome = loop {
        match persist_projection_async(client, messages, &DirectP5ProjectionProvenance::default())
            .await
        {
            Ok(outcome) => break outcome,
            Err(error) if retry < 2 && is_transient_sqlite_lock(&error) => {
                retry += 1;
                tokio::time::sleep(std::time::Duration::from_millis(u64::from(retry) * 100)).await;
            }
            Err(error) => return Err(error),
        }
    };
    if outcome
        .stored_messages
        .saturating_add(outcome.backlogged_messages)
        != messages.len()
    {
        return Err(crate::ImError::LocalProjectionUnavailable {
            detail: "P6 plaintext projection was not durably stored".to_owned(),
        });
    }
    client.emit_committed_local_message_projection(source);
    Ok(())
}

#[cfg(feature = "group-e2ee")]
fn is_transient_sqlite_lock(error: &crate::ImError) -> bool {
    matches!(
        error,
        crate::ImError::LocalStateUnavailable { detail }
            if detail == "database is locked" || detail == "database table is locked"
    )
}

#[cfg(feature = "group-e2ee")]
fn strip_p6_v2_wire_fields(message: &mut Value) {
    let Some(object) = message.as_object_mut() else {
        return;
    };
    for field in [
        "jsonrpc",
        "method",
        "params",
        "meta",
        "body",
        "auth",
        "group_cipher_object",
    ] {
        object.remove(field);
    }
}

#[cfg(feature = "group-e2ee")]
pub(crate) fn clear_untrusted_p6_projection_state(messages: &mut [Value]) {
    for message in messages {
        if !is_p6_v2_projection_candidate(message) {
            continue;
        }
        if let Some(object) = message.as_object_mut() {
            // These fields are local authenticated projection output. A Group
            // Host must not be able to supply them to bypass origin proof,
            // exact-device, or MLS verification.
            for field in [
                "id",
                "message_id",
                "raw_message_id",
                "sender_did",
                "sender_device_id",
                "receiver_did",
                "recipient_did",
                "group_did",
                "group_state_version",
                "group_event_seq",
                "accepted_at",
                "sent_at",
                "direction",
                "secure",
                "decrypted",
                "decryption_state",
                "content",
                "content_type",
                "type",
                "message_security_profile",
                "security",
            ] {
                object.remove(field);
            }
        }
    }
}

#[cfg(feature = "group-e2ee")]
fn p6_projection_error_code(error: &crate::ImError) -> &'static str {
    match error {
        crate::ImError::InvalidInput { .. } => "invalid_wire_or_binding",
        crate::ImError::PermissionDenied => "proof_or_sender_rejected",
        crate::ImError::IdentityUnresolved { .. } | crate::ImError::PeerNotFound { .. } => {
            "sender_document_unavailable"
        }
        crate::ImError::LocalStateUnavailable { .. } => "local_mls_state_unavailable",
        crate::ImError::Internal { message } if message.contains("(group.e2ee.epoch_conflict)") => {
            "mls_epoch_conflict"
        }
        crate::ImError::Internal { message }
            if message.contains("(group.e2ee.private_message_invalid)") =>
        {
            "mls_private_message_invalid"
        }
        crate::ImError::Internal { message }
            if message.contains("(group.e2ee.did_binding_invalid)") =>
        {
            "mls_sender_binding_invalid"
        }
        crate::ImError::Internal { message }
            if message.contains("(group.e2ee.state_not_ready)") =>
        {
            "mls_state_not_ready"
        }
        crate::ImError::Internal { .. } => "mls_processing_failed",
        _ => "projection_dependency_failed",
    }
}

#[cfg(feature = "group-e2ee")]
pub(crate) async fn project_p6_v2_incoming_message(
    client: &crate::core::ImClient,
    message: &mut Value,
) -> crate::ImResult<()> {
    let wrapper_shape = message.get("params").is_some();
    let (meta, body, auth) = parse_p6_v2_incoming_notification(message)?;
    let runtime = crate::internal::group_e2ee::v2_runtime::runtime_for_client(client)?;
    let scope = runtime.owner_scope()?;
    let recipient_device_id = p6_recipient_device_id(
        client.did().as_str(),
        scope.owner_did.as_str(),
        scope.device_id.as_str(),
    )?;
    let sender_document = resolve_direct_sender_document_async(
        client,
        &mut crate::internal::transport::CoreHttpTransport::new(client),
        &meta.sender_did,
    )
    .await?;
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
    let raw_message_id = meta.message_id.clone();
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
    strip_p6_v2_wire_fields(message);
    let object = message
        .as_object_mut()
        .ok_or(crate::ImError::PermissionDenied)?;
    object.insert(
        "id".to_owned(),
        Value::String(p6_group_message_id(&group_did, &group_event_seq)?),
    );
    object.insert(
        "message_id".to_owned(),
        Value::String(raw_message_id.clone()),
    );
    object.insert("raw_message_id".to_owned(), Value::String(raw_message_id));
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
fn p6_group_message_id(group_did: &str, group_event_seq: &str) -> crate::ImResult<String> {
    let group_did = group_did.trim();
    let group_event_seq = group_event_seq.trim();
    if group_did.is_empty() || group_event_seq.is_empty() {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(format!("{group_did}:{group_event_seq}"))
}

#[cfg(feature = "group-e2ee")]
fn p6_recipient_device_id(
    expected_owner_did: &str,
    scope_owner_did: &str,
    scope_device_id: &str,
) -> crate::ImResult<String> {
    if scope_owner_did == expected_owner_did && !scope_device_id.is_empty() {
        Ok(scope_device_id.to_owned())
    } else {
        Err(crate::ImError::PermissionDenied)
    }
}

#[cfg(feature = "group-e2ee")]
fn parse_p6_v2_incoming_notification(
    message: &Value,
) -> crate::ImResult<(
    anp::group_e2ee::V2GroupIncomingMetadata,
    anp::group_e2ee::V2GroupIncomingBody,
    anp::group_e2ee::V2DeliveredOriginAuth,
)> {
    let mut message = message.clone();
    let object = message
        .as_object_mut()
        .ok_or(crate::ImError::PermissionDenied)?;
    if let Some(jsonrpc) = object.remove("jsonrpc") {
        if jsonrpc.as_str() != Some("2.0") {
            return Err(crate::ImError::PermissionDenied);
        }
    }
    let wrapper_shape = object.contains_key("params");
    let notification = if wrapper_shape {
        message
    } else {
        serde_json::json!({
            "method": anp::group_e2ee::METHOD_GROUP_INCOMING_V2,
            "params": {
                "meta": message.get("meta").cloned().ok_or(crate::ImError::PermissionDenied)?,
                "body": message.get("body").cloned().ok_or(crate::ImError::PermissionDenied)?,
                "auth": message.get("auth").cloned().ok_or(crate::ImError::PermissionDenied)?,
            }
        })
    };
    anp::group_e2ee::parse_group_incoming_notification_v2(&notification)
        .map_err(|_| crate::ImError::PermissionDenied)
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
    cache_attachment_manifests_for_internal_download_async(client, std::slice::from_ref(&message))
        .await;
    redact_attachment_manifests_for_public_projection(std::slice::from_mut(&mut message));
    let cached_message = message_from_value(client, &message, None)?.ok_or_else(|| {
        crate::ImError::LocalProjectionUnavailable {
            detail: "P6 realtime plaintext did not produce a local message".to_owned(),
        }
    })?;
    persist_newly_decrypted_p6_messages_async(
        client,
        std::slice::from_ref(&cached_message),
        "p6_realtime_decryption",
    )
    .await?;
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
    raw: &mut Value,
) {
    clear_untrusted_security_markers_in_raw(raw);
}

#[cfg(not(feature = "group-e2ee"))]
pub(crate) async fn project_group_e2ee_messages_for_group_async(
    client: &crate::core::ImClient,
    raw: &mut Value,
    _group_did: &str,
) {
    project_group_e2ee_messages_async(client, raw).await;
}

pub(crate) async fn project_group_e2ee_messages_for_attachment_download_async(
    client: &crate::core::ImClient,
    raw: &mut Value,
) {
    #[cfg(feature = "group-e2ee")]
    project_group_e2ee_messages_async_impl(client, raw, false, None, false).await;
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
    if is_p6_v2_projection_candidate(message) {
        return parse_p6_v2_incoming_notification(message)
            .ok()
            .and_then(|(_, body, _)| {
                p6_group_message_id(&body.group_did, &body.group_event_seq).ok()
            })
            .unwrap_or_default();
    }
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
) -> HashSet<usize> {
    let message_ids = group_e2ee_wire_message_ids(messages);
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
    apply_cached_group_e2ee_records_for_owner(messages, records, client.did().as_str())
}

#[cfg(all(
    feature = "sqlite",
    feature = "group-e2ee",
    not(any(feature = "blocking", test))
))]
pub(crate) fn apply_cached_group_e2ee_messages(
    _client: &crate::core::ImClient,
    _messages: &mut [Value],
) -> HashSet<usize> {
    HashSet::new()
}

#[cfg(all(feature = "sqlite", feature = "group-e2ee"))]
pub(crate) async fn apply_cached_group_e2ee_messages_async(
    client: &crate::core::ImClient,
    messages: &mut [Value],
) -> HashSet<usize> {
    let message_ids = group_e2ee_wire_message_ids(messages);
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
    apply_cached_group_e2ee_records_for_owner(messages, records, client.did().as_str())
}

#[cfg(all(feature = "group-e2ee", not(feature = "sqlite")))]
pub(crate) async fn apply_cached_group_e2ee_messages_async(
    _client: &crate::core::ImClient,
    _messages: &mut [Value],
) -> HashSet<usize> {
    HashSet::new()
}

#[cfg(all(feature = "sqlite", feature = "group-e2ee"))]
fn apply_cached_group_e2ee_records_for_owner(
    messages: &mut [Value],
    records: Vec<crate::internal::local_state::messages::MessageRecord>,
    expected_owner_did: &str,
) -> HashSet<usize> {
    let records = records
        .into_iter()
        .map(|record| (record.msg_id.clone(), record))
        .collect::<HashMap<_, _>>();
    let mut applied = HashSet::new();
    for (index, message) in messages.iter_mut().enumerate() {
        let message_id = group_e2ee_message_cache_id(message);
        let Some(record) = records.get(&message_id) else {
            continue;
        };
        if is_p6_v2_projection_candidate(message)
            && !p6_cached_record_matches_wire(message, record, expected_owner_did)
        {
            continue;
        }
        if crate::internal::group_e2ee::incoming::apply_cached_group_plaintext(
            message,
            &record.content_type,
            &record.content,
        ) {
            if let Some(object) = message.as_object_mut() {
                object.insert("id".to_owned(), Value::String(record.msg_id.clone()));
                object.insert(
                    "sender_did".to_owned(),
                    Value::String(record.sender_did.clone()),
                );
                object.insert(
                    "group_did".to_owned(),
                    Value::String(record.group_did.clone()),
                );
                object.insert(
                    "receiver_did".to_owned(),
                    Value::String(record.owner_did.clone()),
                );
                object.insert("direction".to_owned(), Value::from(record.direction));
                if let Some(server_seq) = record.server_seq {
                    object.insert("group_event_seq".to_owned(), Value::from(server_seq));
                }
                if !record.sent_at.trim().is_empty() {
                    object.insert("sent_at".to_owned(), Value::String(record.sent_at.clone()));
                }
                object.insert("is_read".to_owned(), Value::Bool(record.is_read));
            }
            applied.insert(index);
        }
    }
    applied
}

#[cfg(all(feature = "sqlite", feature = "group-e2ee"))]
fn p6_cached_record_matches_wire(
    message: &Value,
    record: &crate::internal::local_state::messages::MessageRecord,
    expected_owner_did: &str,
) -> bool {
    let Ok((meta, body, _)) = parse_p6_v2_incoming_notification(message) else {
        return false;
    };
    let Ok(message_id) = p6_group_message_id(&body.group_did, &body.group_event_seq) else {
        return false;
    };
    record.is_e2ee
        && record.msg_id == message_id
        && record.group_did == body.group_did
        && (record.group_id.is_empty() || record.group_id == body.group_did)
        && record.sender_did == meta.sender_did
        && record.owner_did == meta.target.did
        && (expected_owner_did.is_empty() || record.owner_did == expected_owner_did)
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
fn apply_cached_group_attachment_manifests(client: &crate::core::ImClient, messages: &mut [Value]) {
    let Ok(connection) = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    ) else {
        return;
    };
    let owner_identity_id = client.current_identity().id.as_str();
    for message in messages {
        let Some(object) = message.as_object() else {
            continue;
        };
        if object.get("content_type").and_then(Value::as_str)
            != Some("application/x-awiki-group-e2ee-redacted")
            || object.get("decryption_state").and_then(Value::as_str) != Some("failed")
        {
            continue;
        }
        let Some(group_did) = first_non_empty_owned([
            string_value(object.get("group_did")),
            string_value(object.get("group")),
        ]) else {
            continue;
        };
        let Some(message_id) = attachment_manifest_cache_message_id(object, &group_did) else {
            continue;
        };
        let Ok(Some(cached)) = crate::internal::local_state::attachment_manifest_cache::get_attachment_manifest_cache_message(
            &connection,
            owner_identity_id,
            "group",
            &group_did,
            &message_id,
        ) else {
            continue;
        };
        let (Some(target), Some(cached)) = (message.as_object_mut(), cached.as_object()) else {
            continue;
        };
        target.extend(cached.clone());
    }
}

#[cfg(not(feature = "sqlite"))]
fn apply_cached_group_attachment_manifests(
    _client: &crate::core::ImClient,
    _messages: &mut [Value],
) {
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
            wire_message_id: string_value(object.get("raw_message_id")),
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

pub(crate) fn retain_direct_messages_for_expected_peer(
    client: &crate::core::ImClient,
    raw: &mut Value,
    expected_peer_did: &str,
    p5_provenance: &mut DirectP5ProjectionProvenance,
) -> bool {
    let expected_peer_did = expected_peer_did.trim();
    p5_provenance.expected_peer_did = Some(expected_peer_did.to_owned());
    let Some(messages) = raw.get_mut("messages").and_then(Value::as_array_mut) else {
        return false;
    };
    retain_direct_message_values_for_expected_peer(client, messages, expected_peer_did)
}

fn retain_direct_message_values_for_expected_peer(
    client: &crate::core::ImClient,
    messages: &mut Vec<Value>,
    expected_peer_did: &str,
) -> bool {
    let expected_peer_did = expected_peer_did.trim();
    let original_len = messages.len();
    messages.retain(|message| {
        let Some(object) = message.as_object() else {
            return false;
        };
        if !string_value(object.get("group_did")).trim().is_empty() {
            return false;
        }
        let sender_did = string_value(object.get("sender_did"));
        let receiver_did = string_value(object.get("receiver_did"));
        direct_peer_did_for_message(client.did().as_str(), &sender_did, &receiver_did).trim()
            == expected_peer_did
    });
    original_len > 0 && messages.is_empty()
}

pub(crate) fn reject_stalled_scoped_direct_page(
    raw: &Value,
    final_projectable_count: usize,
) -> crate::ImResult<()> {
    if final_projectable_count == 0 && raw.get("has_more").and_then(Value::as_bool) == Some(true) {
        return Err(scoped_direct_page_mismatch());
    }
    Ok(())
}

fn scoped_direct_page_mismatch() -> crate::ImError {
    crate::ImError::IdentityBindingConflict {
        detail: "Direct page does not match the requested peer scope".to_owned(),
    }
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
            value: raw_message_id.clone(),
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
