use std::path::PathBuf;

use tokio::sync::{mpsc, oneshot};

const COMMAND_BUFFER: usize = 64;

#[derive(Clone)]
pub(crate) struct LocalStateDb {
    sender: mpsc::Sender<LocalStateCommand>,
}

enum LocalStateCommand {
    CurrentSchemaVersion {
        reply: oneshot::Sender<crate::ImResult<i64>>,
    },
    EnsureConversation {
        owner_identity_id: String,
        owner_did: String,
        conversation_id: String,
        reply: oneshot::Sender<crate::ImResult<()>>,
    },
    StoreMessages {
        records: Vec<super::messages::MessageRecord>,
        reply: oneshot::Sender<crate::ImResult<()>>,
    },
    StoreRemoteMessages {
        records: Vec<super::messages::MessageRecord>,
        source_event_type: String,
        reply: oneshot::Sender<
            crate::ImResult<super::inbound_resolution_backlog::RemoteMessageIngestOutcome>,
        >,
    },
    LoadGlobalCheckpoint {
        owner_identity_id: String,
        reply: oneshot::Sender<crate::ImResult<Option<super::sync_state::GlobalCheckpoint>>>,
    },
    ApplySyncDelta {
        input: super::sync_state::SyncDeltaApplyInput,
        reply: oneshot::Sender<crate::ImResult<super::sync_state::SyncDeltaApplyOutcome>>,
    },
    ApplySystemNotification {
        input: crate::internal::system_notification::store::SystemNotificationApplyInput,
        reply: oneshot::Sender<
            crate::ImResult<
                crate::internal::system_notification::store::SystemNotificationApplyOutcome,
            >,
        >,
    },
    ListSystemNotifications {
        owner_identity_id: String,
        owner_did: String,
        protocol_device_id: String,
        include_terminal: bool,
        limit: u32,
        reply: oneshot::Sender<
            crate::ImResult<Vec<crate::system_notifications::SystemNotificationSnapshot>>,
        >,
    },
    GetSystemNotification {
        owner_identity_id: String,
        owner_did: String,
        protocol_device_id: String,
        event_id: String,
        reply: oneshot::Sender<
            crate::ImResult<Option<crate::system_notifications::SystemNotificationSnapshot>>,
        >,
    },
    ListVerifiedSystemNotifications {
        owner_identity_id: String,
        owner_did: String,
        protocol_device_id: String,
        include_terminal: bool,
        limit: u32,
        reply: oneshot::Sender<
            crate::ImResult<Vec<crate::internal::system_notification::wire::JoinNotification>>,
        >,
    },
    GetVerifiedSystemNotification {
        owner_identity_id: String,
        owner_did: String,
        protocol_device_id: String,
        join_session_id: String,
        reply: oneshot::Sender<
            crate::ImResult<Option<crate::internal::system_notification::wire::JoinNotification>>,
        >,
    },
    UpsertContact {
        record: crate::internal::contact_store::records::ContactRecord,
        reply: oneshot::Sender<crate::ImResult<()>>,
    },
    UpsertDirectPeerRoute {
        record: super::direct_peer_routes::DirectPeerRouteRecord,
        reply: oneshot::Sender<crate::ImResult<()>>,
    },
    ProjectVerifiedHandle {
        owner_identity_id: String,
        owner_did: String,
        lookup: crate::directory::HandleLookupResult,
        reply: oneshot::Sender<crate::ImResult<String>>,
    },
    GetPersonaDisplayProfile {
        owner_identity_id: String,
        peer: crate::ids::PeerRef,
        reply: oneshot::Sender<crate::ImResult<Option<crate::directory::DisplayProfile>>>,
    },
    GetContactByDid {
        owner_identity_id: String,
        owner_did: String,
        did: String,
        reply: oneshot::Sender<
            crate::ImResult<crate::internal::contact_store::records::ContactRecord>,
        >,
    },
    GetCurrentContactByHandle {
        owner_identity_id: String,
        owner_did: String,
        handle: String,
        reply: oneshot::Sender<
            crate::ImResult<crate::internal::contact_store::records::ContactRecord>,
        >,
    },
    ListContacts {
        owner_identity_id: String,
        owner_did: String,
        limit: i64,
        reply: oneshot::Sender<
            crate::ImResult<Vec<crate::internal::contact_store::records::ContactRecord>>,
        >,
    },
    ListContactDidsForMessageHistoryRecovery {
        owner_identity_id: String,
        owner_did: String,
        limit: i64,
        reply: oneshot::Sender<crate::ImResult<Vec<String>>>,
    },
    ResolveContactHandleByDid {
        owner_identity_id: String,
        owner_did: String,
        did: String,
        reply: oneshot::Sender<crate::ImResult<String>>,
    },
    ListDidsByHandle {
        owner_identity_id: String,
        owner_did: String,
        handle: String,
        reply: oneshot::Sender<crate::ImResult<Vec<String>>>,
    },
    AppendRelationshipEvent {
        record: crate::internal::contact_store::records::RelationshipEventRecord,
        reply: oneshot::Sender<crate::ImResult<String>>,
    },
    UpsertGroup {
        record: super::groups::GroupRecord,
        reply: oneshot::Sender<crate::ImResult<()>>,
    },
    UpsertGroupE2eeSummary {
        summary: super::groups::GroupE2eeSummaryRecord,
        reply: oneshot::Sender<crate::ImResult<()>>,
    },
    ReplaceGroupMembers {
        owner_identity_id: String,
        owner_did: String,
        group_id: String,
        members: Vec<super::groups::GroupMemberRecord>,
        credential_name: String,
        reply: oneshot::Sender<crate::ImResult<()>>,
    },
    MarkGroupLeft {
        owner_identity_id: String,
        owner_did: String,
        group_id: String,
        group_did: String,
        credential_name: String,
        reply: oneshot::Sender<crate::ImResult<()>>,
    },
    GetGroupSnapshot {
        owner_identity_id: String,
        owner_did: String,
        group_id: String,
        reply: oneshot::Sender<crate::ImResult<Option<serde_json::Value>>>,
    },
    ListCachedGroupMembers {
        owner_identity_id: String,
        owner_did: String,
        group_id: String,
        limit: i64,
        reply: oneshot::Sender<crate::ImResult<Vec<serde_json::Value>>>,
    },
    ListGroupMessages {
        owner_identity_id: String,
        owner_did: String,
        group_id: String,
        limit: i64,
        since_seq: Option<i64>,
        reply: oneshot::Sender<crate::ImResult<Vec<serde_json::Value>>>,
    },
    ListDirectMessages {
        owner_identity_id: String,
        conversation_ids: Vec<String>,
        limit: i64,
        reply: oneshot::Sender<crate::ImResult<Vec<super::messages::MessageRecord>>>,
    },
    ListDecryptedSecureMessages {
        owner_identity_id: String,
        message_ids: Vec<String>,
        reply: oneshot::Sender<crate::ImResult<Vec<super::messages::MessageRecord>>>,
    },
    ListMessagesForThreadRef {
        owner_identity_id: String,
        owner_did: String,
        thread: crate::messages::ThreadRef,
        limit: i64,
        cursor: Option<String>,
        reply: oneshot::Sender<crate::ImResult<super::messages::ThreadLocalHistoryRecords>>,
    },
    MaxServerSeqForThreadRef {
        owner_identity_id: String,
        owner_did: String,
        thread: crate::messages::ThreadRef,
        reply: oneshot::Sender<crate::ImResult<Option<i64>>>,
    },
    MessageServerSeq {
        owner_identity_id: String,
        message_id: String,
        reply: oneshot::Sender<crate::ImResult<Option<i64>>>,
    },
    ListActiveGroupRefs {
        owner_identity_id: String,
        owner_did: String,
        limit: i64,
        reply: oneshot::Sender<crate::ImResult<Vec<String>>>,
    },
    ClassifyMarkReadIds {
        owner_identity_id: String,
        owner_did: String,
        message_ids: Vec<String>,
        reply: oneshot::Sender<crate::ImResult<super::messages::MarkReadClassification>>,
    },
    ListUnreadIncomingMessageIds {
        owner_identity_id: String,
        owner_did: String,
        thread: crate::messages::ThreadRef,
        limit: i64,
        reply: oneshot::Sender<crate::ImResult<super::messages::ThreadUnreadMessageIds>>,
    },
    ListIncomingMessageIdsUpToWatermark {
        owner_identity_id: String,
        owner_did: String,
        thread: crate::messages::ThreadRef,
        read_watermark_message_id: Option<String>,
        read_watermark_seq: Option<String>,
        limit: i64,
        reply: oneshot::Sender<crate::ImResult<super::messages::ThreadUnreadMessageIds>>,
    },
    MarkMessagesRead {
        owner_identity_id: String,
        owner_did: String,
        message_ids: Vec<String>,
        reply: oneshot::Sender<crate::ImResult<i64>>,
    },
    MarkThreadReadWatermark {
        owner_identity_id: String,
        owner_did: String,
        input: super::messages::MarkThreadReadWatermarkInput,
        reply: oneshot::Sender<crate::ImResult<super::messages::MarkThreadReadWatermarkResult>>,
    },
    ListConversations {
        owner_identity_id: String,
        owner_did: String,
        query: crate::messages::ConversationQuery,
        reply: oneshot::Sender<crate::ImResult<Vec<super::conversations::ConversationRecord>>>,
    },
    ListMailNotifications {
        owner_identity_id: String,
        owner_did: String,
        limit: crate::ids::PageLimit,
        reply: oneshot::Sender<crate::ImResult<crate::ids::Page<crate::email::EmailNotification>>>,
    },
    ListE2eeOutbox {
        scope: crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope,
        local_status: Option<String>,
        reply: oneshot::Sender<
            crate::ImResult<Vec<crate::internal::store::e2ee_outbox::E2eeOutboxRecord>>,
        >,
    },
    QueueE2eeOutbox {
        record: crate::internal::store::e2ee_outbox::E2eeOutboxRecord,
        reply: oneshot::Sender<crate::ImResult<String>>,
    },
    MarkE2eeOutboxSent {
        scope: crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope,
        outbox_id: String,
        session_id: String,
        sent_msg_id: String,
        sent_server_seq: Option<i64>,
        metadata: String,
        reply: oneshot::Sender<crate::ImResult<()>>,
    },
    MarkE2eeOutboxSentAndStoreMessage {
        scope: crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope,
        outbox_id: String,
        session_id: String,
        sent_msg_id: String,
        sent_server_seq: Option<i64>,
        metadata: String,
        message: super::messages::MessageRecord,
        reply: oneshot::Sender<crate::ImResult<()>>,
    },
    SetE2eeOutboxFailure {
        scope: crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope,
        outbox_id: String,
        error_code: String,
        retry_hint: String,
        metadata: String,
        reply: oneshot::Sender<crate::ImResult<()>>,
    },
    DirectSecureStatus {
        scope: crate::internal::secure_direct::status::DirectSecureStatusScope,
        peer: crate::ids::PeerRef,
        reply: oneshot::Sender<
            crate::ImResult<crate::internal::secure_direct::status::DirectSecureLocalStatus>,
        >,
    },
    DirectSecureRepair {
        scope: crate::internal::secure_direct::status::DirectSecureStatusScope,
        peer: crate::ids::PeerRef,
        reply: oneshot::Sender<
            crate::ImResult<crate::internal::secure_direct::status::DirectSecureRepairPlan>,
        >,
    },
    PrepareDirectSecurePrekeys {
        input: crate::internal::secure_direct::prepare::DirectSecurePrekeyPrepareInput,
        reply: oneshot::Sender<
            crate::ImResult<
                crate::internal::secure_direct::prepare::DirectSecurePrekeyPrepareResult,
            >,
        >,
    },
    GetDirectSecureSession {
        owner_identity_id: String,
        peer_did: String,
        reply: oneshot::Sender<
            crate::ImResult<
                Option<crate::internal::secure_direct::sqlite_store::DirectSessionRecord>,
            >,
        >,
    },
    DirectInitSessionMaterial {
        owner_identity_id: String,
        peer_did: String,
        session_id: String,
        signed_prekey_id: String,
        one_time_prekey_id: Option<String>,
        reply: oneshot::Sender<
            crate::ImResult<
                crate::internal::secure_direct::sqlite_store::DirectInitSessionMaterial,
            >,
        >,
    },
    SaveIncomingDirectInitSession {
        commit: crate::internal::secure_direct::sqlite_store::DirectInitSessionCommit,
        reply: oneshot::Sender<
            crate::ImResult<
                crate::internal::secure_direct::sqlite_store::DirectInitSessionCommitResult,
            >,
        >,
    },
    SaveOutgoingDirectInitSession {
        commit: crate::internal::secure_direct::sqlite_store::DirectInitSendCommit,
        reply: oneshot::Sender<
            crate::ImResult<
                crate::internal::secure_direct::sqlite_store::DirectInitSessionCommitResult,
            >,
        >,
    },
    SaveDirectSecureSessionIfRevision {
        record: crate::internal::secure_direct::sqlite_store::DirectSessionRecord,
        expected_revision: i64,
        reply: oneshot::Sender<
            crate::ImResult<crate::internal::secure_direct::sqlite_store::DirectSessionCasResult>,
        >,
    },
    RetryE2eeOutbox {
        scope: crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope,
        outbox_id: String,
        reply: oneshot::Sender<
            crate::ImResult<Option<crate::internal::store::e2ee_outbox::E2eeOutboxRecord>>,
        >,
    },
    DropE2eeOutbox {
        scope: crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope,
        outbox_id: String,
        reply: oneshot::Sender<
            crate::ImResult<Option<crate::internal::store::e2ee_outbox::E2eeOutboxRecord>>,
        >,
    },
    BackfillOwnerIdentityIds {
        identities: Vec<super::schema::OwnerIdentityBackfill>,
        reply: oneshot::Sender<crate::ImResult<usize>>,
    },
    Shutdown {
        reply: oneshot::Sender<()>,
    },
}

impl LocalStateDb {
    pub(crate) async fn ensure_conversation(
        &self,
        owner_identity_id: impl Into<String>,
        owner_did: impl Into<String>,
        conversation_id: impl Into<String>,
    ) -> crate::ImResult<()> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::EnsureConversation {
            owner_identity_id: owner_identity_id.into(),
            owner_did: owner_did.into(),
            conversation_id: conversation_id.into(),
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn open(sqlite_path: PathBuf) -> crate::ImResult<Self> {
        let (sender, receiver) = mpsc::channel(COMMAND_BUFFER);
        let (ready_sender, ready_receiver) = oneshot::channel();
        std::thread::Builder::new()
            .name("im-core-local-state-db".to_owned())
            .spawn(move || run_actor(sqlite_path, receiver, ready_sender))
            .map_err(|err| crate::ImError::Internal {
                message: format!("spawn local state actor: {err}"),
            })?;
        ready_receiver.await.map_err(|_| actor_closed())??;
        Ok(Self { sender })
    }

    pub(crate) async fn current_schema_version(&self) -> crate::ImResult<i64> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::CurrentSchemaVersion { reply })
            .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn store_messages(
        &self,
        records: Vec<super::messages::MessageRecord>,
    ) -> crate::ImResult<()> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::StoreMessages { records, reply })
            .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn store_remote_messages(
        &self,
        records: Vec<super::messages::MessageRecord>,
        source_event_type: impl Into<String>,
    ) -> crate::ImResult<super::inbound_resolution_backlog::RemoteMessageIngestOutcome> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::StoreRemoteMessages {
            records,
            source_event_type: source_event_type.into(),
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn load_global_checkpoint(
        &self,
        owner_identity_id: impl Into<String>,
    ) -> crate::ImResult<Option<super::sync_state::GlobalCheckpoint>> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::LoadGlobalCheckpoint {
            owner_identity_id: owner_identity_id.into(),
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn apply_sync_delta(
        &self,
        input: super::sync_state::SyncDeltaApplyInput,
    ) -> crate::ImResult<super::sync_state::SyncDeltaApplyOutcome> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::ApplySyncDelta { input, reply })
            .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn apply_system_notification(
        &self,
        input: crate::internal::system_notification::store::SystemNotificationApplyInput,
    ) -> crate::ImResult<crate::internal::system_notification::store::SystemNotificationApplyOutcome>
    {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::ApplySystemNotification { input, reply })
            .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn list_system_notifications(
        &self,
        owner_identity_id: impl Into<String>,
        owner_did: impl Into<String>,
        protocol_device_id: impl Into<String>,
        include_terminal: bool,
        limit: u32,
    ) -> crate::ImResult<Vec<crate::system_notifications::SystemNotificationSnapshot>> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::ListSystemNotifications {
            owner_identity_id: owner_identity_id.into(),
            owner_did: owner_did.into(),
            protocol_device_id: protocol_device_id.into(),
            include_terminal,
            limit,
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn get_system_notification(
        &self,
        owner_identity_id: impl Into<String>,
        owner_did: impl Into<String>,
        protocol_device_id: impl Into<String>,
        event_id: impl Into<String>,
    ) -> crate::ImResult<Option<crate::system_notifications::SystemNotificationSnapshot>> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::GetSystemNotification {
            owner_identity_id: owner_identity_id.into(),
            owner_did: owner_did.into(),
            protocol_device_id: protocol_device_id.into(),
            event_id: event_id.into(),
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn list_verified_system_notifications(
        &self,
        owner_identity_id: impl Into<String>,
        owner_did: impl Into<String>,
        protocol_device_id: impl Into<String>,
        include_terminal: bool,
        limit: u32,
    ) -> crate::ImResult<Vec<crate::internal::system_notification::wire::JoinNotification>> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::ListVerifiedSystemNotifications {
            owner_identity_id: owner_identity_id.into(),
            owner_did: owner_did.into(),
            protocol_device_id: protocol_device_id.into(),
            include_terminal,
            limit,
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn get_verified_system_notification(
        &self,
        owner_identity_id: impl Into<String>,
        owner_did: impl Into<String>,
        protocol_device_id: impl Into<String>,
        join_session_id: impl Into<String>,
    ) -> crate::ImResult<Option<crate::internal::system_notification::wire::JoinNotification>> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::GetVerifiedSystemNotification {
            owner_identity_id: owner_identity_id.into(),
            owner_did: owner_did.into(),
            protocol_device_id: protocol_device_id.into(),
            join_session_id: join_session_id.into(),
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn upsert_contact(
        &self,
        record: crate::internal::contact_store::records::ContactRecord,
    ) -> crate::ImResult<()> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::UpsertContact { record, reply })
            .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn upsert_direct_peer_route(
        &self,
        record: super::direct_peer_routes::DirectPeerRouteRecord,
    ) -> crate::ImResult<()> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::UpsertDirectPeerRoute { record, reply })
            .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn project_verified_handle(
        &self,
        owner_identity_id: impl Into<String>,
        owner_did: impl Into<String>,
        lookup: crate::directory::HandleLookupResult,
    ) -> crate::ImResult<String> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::ProjectVerifiedHandle {
            owner_identity_id: owner_identity_id.into(),
            owner_did: owner_did.into(),
            lookup,
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn get_persona_display_profile(
        &self,
        owner_identity_id: impl Into<String>,
        peer: crate::ids::PeerRef,
    ) -> crate::ImResult<Option<crate::directory::DisplayProfile>> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::GetPersonaDisplayProfile {
            owner_identity_id: owner_identity_id.into(),
            peer,
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn get_contact_by_did(
        &self,
        owner_identity_id: impl Into<String>,
        owner_did: impl Into<String>,
        did: impl Into<String>,
    ) -> crate::ImResult<crate::internal::contact_store::records::ContactRecord> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::GetContactByDid {
            owner_identity_id: owner_identity_id.into(),
            owner_did: owner_did.into(),
            did: did.into(),
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn get_current_contact_by_handle(
        &self,
        owner_identity_id: impl Into<String>,
        owner_did: impl Into<String>,
        handle: impl Into<String>,
    ) -> crate::ImResult<crate::internal::contact_store::records::ContactRecord> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::GetCurrentContactByHandle {
            owner_identity_id: owner_identity_id.into(),
            owner_did: owner_did.into(),
            handle: handle.into(),
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn list_contacts(
        &self,
        owner_identity_id: impl Into<String>,
        owner_did: impl Into<String>,
        limit: i64,
    ) -> crate::ImResult<Vec<crate::internal::contact_store::records::ContactRecord>> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::ListContacts {
            owner_identity_id: owner_identity_id.into(),
            owner_did: owner_did.into(),
            limit,
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn list_contact_dids_for_message_history_recovery(
        &self,
        owner_identity_id: impl Into<String>,
        owner_did: impl Into<String>,
        limit: i64,
    ) -> crate::ImResult<Vec<String>> {
        let (reply, receiver) = oneshot::channel();
        self.send(
            LocalStateCommand::ListContactDidsForMessageHistoryRecovery {
                owner_identity_id: owner_identity_id.into(),
                owner_did: owner_did.into(),
                limit,
                reply,
            },
        )
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn resolve_contact_handle_by_did(
        &self,
        owner_identity_id: impl Into<String>,
        owner_did: impl Into<String>,
        did: impl Into<String>,
    ) -> crate::ImResult<String> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::ResolveContactHandleByDid {
            owner_identity_id: owner_identity_id.into(),
            owner_did: owner_did.into(),
            did: did.into(),
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn list_dids_by_handle(
        &self,
        owner_identity_id: impl Into<String>,
        owner_did: impl Into<String>,
        handle: impl Into<String>,
    ) -> crate::ImResult<Vec<String>> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::ListDidsByHandle {
            owner_identity_id: owner_identity_id.into(),
            owner_did: owner_did.into(),
            handle: handle.into(),
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn append_relationship_event(
        &self,
        record: crate::internal::contact_store::records::RelationshipEventRecord,
    ) -> crate::ImResult<String> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::AppendRelationshipEvent { record, reply })
            .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn upsert_group(
        &self,
        record: super::groups::GroupRecord,
    ) -> crate::ImResult<()> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::UpsertGroup { record, reply })
            .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn upsert_group_e2ee_summary(
        &self,
        summary: super::groups::GroupE2eeSummaryRecord,
    ) -> crate::ImResult<()> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::UpsertGroupE2eeSummary { summary, reply })
            .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn replace_group_members(
        &self,
        owner_identity_id: impl Into<String>,
        owner_did: impl Into<String>,
        group_id: impl Into<String>,
        members: Vec<super::groups::GroupMemberRecord>,
        credential_name: impl Into<String>,
    ) -> crate::ImResult<()> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::ReplaceGroupMembers {
            owner_identity_id: owner_identity_id.into(),
            owner_did: owner_did.into(),
            group_id: group_id.into(),
            members,
            credential_name: credential_name.into(),
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn mark_group_left(
        &self,
        owner_identity_id: impl Into<String>,
        owner_did: impl Into<String>,
        group_id: impl Into<String>,
        group_did: impl Into<String>,
        credential_name: impl Into<String>,
    ) -> crate::ImResult<()> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::MarkGroupLeft {
            owner_identity_id: owner_identity_id.into(),
            owner_did: owner_did.into(),
            group_id: group_id.into(),
            group_did: group_did.into(),
            credential_name: credential_name.into(),
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn get_group_snapshot(
        &self,
        owner_identity_id: impl Into<String>,
        owner_did: impl Into<String>,
        group_id: impl Into<String>,
    ) -> crate::ImResult<Option<serde_json::Value>> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::GetGroupSnapshot {
            owner_identity_id: owner_identity_id.into(),
            owner_did: owner_did.into(),
            group_id: group_id.into(),
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn list_cached_group_members(
        &self,
        owner_identity_id: impl Into<String>,
        owner_did: impl Into<String>,
        group_id: impl Into<String>,
        limit: i64,
    ) -> crate::ImResult<Vec<serde_json::Value>> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::ListCachedGroupMembers {
            owner_identity_id: owner_identity_id.into(),
            owner_did: owner_did.into(),
            group_id: group_id.into(),
            limit,
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn list_group_messages(
        &self,
        owner_identity_id: impl Into<String>,
        owner_did: impl Into<String>,
        group_id: impl Into<String>,
        limit: i64,
        since_seq: Option<i64>,
    ) -> crate::ImResult<Vec<serde_json::Value>> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::ListGroupMessages {
            owner_identity_id: owner_identity_id.into(),
            owner_did: owner_did.into(),
            group_id: group_id.into(),
            limit,
            since_seq,
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn list_direct_messages(
        &self,
        owner_identity_id: impl Into<String>,
        conversation_ids: Vec<String>,
        limit: i64,
    ) -> crate::ImResult<Vec<super::messages::MessageRecord>> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::ListDirectMessages {
            owner_identity_id: owner_identity_id.into(),
            conversation_ids,
            limit,
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn list_decrypted_secure_messages(
        &self,
        owner_identity_id: impl Into<String>,
        message_ids: Vec<String>,
    ) -> crate::ImResult<Vec<super::messages::MessageRecord>> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::ListDecryptedSecureMessages {
            owner_identity_id: owner_identity_id.into(),
            message_ids,
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn list_messages_for_thread_ref(
        &self,
        owner_identity_id: impl Into<String>,
        owner_did: impl Into<String>,
        thread: crate::messages::ThreadRef,
        limit: i64,
        cursor: Option<String>,
    ) -> crate::ImResult<super::messages::ThreadLocalHistoryRecords> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::ListMessagesForThreadRef {
            owner_identity_id: owner_identity_id.into(),
            owner_did: owner_did.into(),
            thread,
            limit,
            cursor,
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn max_server_seq_for_thread_ref(
        &self,
        owner_identity_id: impl Into<String>,
        owner_did: impl Into<String>,
        thread: crate::messages::ThreadRef,
    ) -> crate::ImResult<Option<i64>> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::MaxServerSeqForThreadRef {
            owner_identity_id: owner_identity_id.into(),
            owner_did: owner_did.into(),
            thread,
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn message_server_seq(
        &self,
        owner_identity_id: impl Into<String>,
        message_id: impl Into<String>,
    ) -> crate::ImResult<Option<i64>> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::MessageServerSeq {
            owner_identity_id: owner_identity_id.into(),
            message_id: message_id.into(),
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn list_active_group_refs(
        &self,
        owner_identity_id: impl Into<String>,
        owner_did: impl Into<String>,
        limit: i64,
    ) -> crate::ImResult<Vec<String>> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::ListActiveGroupRefs {
            owner_identity_id: owner_identity_id.into(),
            owner_did: owner_did.into(),
            limit,
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn classify_mark_read_ids(
        &self,
        owner_identity_id: impl Into<String>,
        owner_did: impl Into<String>,
        message_ids: Vec<String>,
    ) -> crate::ImResult<super::messages::MarkReadClassification> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::ClassifyMarkReadIds {
            owner_identity_id: owner_identity_id.into(),
            owner_did: owner_did.into(),
            message_ids,
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn list_unread_incoming_message_ids(
        &self,
        owner_identity_id: impl Into<String>,
        owner_did: impl Into<String>,
        thread: crate::messages::ThreadRef,
        limit: i64,
    ) -> crate::ImResult<super::messages::ThreadUnreadMessageIds> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::ListUnreadIncomingMessageIds {
            owner_identity_id: owner_identity_id.into(),
            owner_did: owner_did.into(),
            thread,
            limit,
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn list_incoming_message_ids_up_to_watermark(
        &self,
        owner_identity_id: impl Into<String>,
        owner_did: impl Into<String>,
        thread: crate::messages::ThreadRef,
        read_watermark_message_id: Option<String>,
        read_watermark_seq: Option<String>,
        limit: i64,
    ) -> crate::ImResult<super::messages::ThreadUnreadMessageIds> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::ListIncomingMessageIdsUpToWatermark {
            owner_identity_id: owner_identity_id.into(),
            owner_did: owner_did.into(),
            thread,
            read_watermark_message_id,
            read_watermark_seq,
            limit,
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn mark_messages_read(
        &self,
        owner_identity_id: impl Into<String>,
        owner_did: impl Into<String>,
        message_ids: Vec<String>,
    ) -> crate::ImResult<i64> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::MarkMessagesRead {
            owner_identity_id: owner_identity_id.into(),
            owner_did: owner_did.into(),
            message_ids,
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn mark_thread_read_watermark(
        &self,
        owner_identity_id: impl Into<String>,
        owner_did: impl Into<String>,
        input: super::messages::MarkThreadReadWatermarkInput,
    ) -> crate::ImResult<super::messages::MarkThreadReadWatermarkResult> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::MarkThreadReadWatermark {
            owner_identity_id: owner_identity_id.into(),
            owner_did: owner_did.into(),
            input,
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn list_conversations(
        &self,
        owner_identity_id: impl Into<String>,
        owner_did: impl Into<String>,
        query: crate::messages::ConversationQuery,
    ) -> crate::ImResult<Vec<super::conversations::ConversationRecord>> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::ListConversations {
            owner_identity_id: owner_identity_id.into(),
            owner_did: owner_did.into(),
            query,
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn list_mail_notifications(
        &self,
        owner_identity_id: impl Into<String>,
        owner_did: impl Into<String>,
        limit: crate::ids::PageLimit,
    ) -> crate::ImResult<crate::ids::Page<crate::email::EmailNotification>> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::ListMailNotifications {
            owner_identity_id: owner_identity_id.into(),
            owner_did: owner_did.into(),
            limit,
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn list_e2ee_outbox(
        &self,
        scope: crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope,
        local_status: Option<String>,
    ) -> crate::ImResult<Vec<crate::internal::store::e2ee_outbox::E2eeOutboxRecord>> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::ListE2eeOutbox {
            scope,
            local_status,
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn queue_e2ee_outbox(
        &self,
        record: crate::internal::store::e2ee_outbox::E2eeOutboxRecord,
    ) -> crate::ImResult<String> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::QueueE2eeOutbox { record, reply })
            .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn mark_e2ee_outbox_sent(
        &self,
        scope: crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope,
        outbox_id: impl Into<String>,
        session_id: impl Into<String>,
        sent_msg_id: impl Into<String>,
        sent_server_seq: Option<i64>,
        metadata: impl Into<String>,
    ) -> crate::ImResult<()> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::MarkE2eeOutboxSent {
            scope,
            outbox_id: outbox_id.into(),
            session_id: session_id.into(),
            sent_msg_id: sent_msg_id.into(),
            sent_server_seq,
            metadata: metadata.into(),
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn mark_e2ee_outbox_sent_and_store_message(
        &self,
        scope: crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope,
        outbox_id: impl Into<String>,
        session_id: impl Into<String>,
        sent_msg_id: impl Into<String>,
        sent_server_seq: Option<i64>,
        metadata: impl Into<String>,
        message: super::messages::MessageRecord,
    ) -> crate::ImResult<()> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::MarkE2eeOutboxSentAndStoreMessage {
            scope,
            outbox_id: outbox_id.into(),
            session_id: session_id.into(),
            sent_msg_id: sent_msg_id.into(),
            sent_server_seq,
            metadata: metadata.into(),
            message,
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn set_e2ee_outbox_failure(
        &self,
        scope: crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope,
        outbox_id: impl Into<String>,
        error_code: impl Into<String>,
        retry_hint: impl Into<String>,
        metadata: impl Into<String>,
    ) -> crate::ImResult<()> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::SetE2eeOutboxFailure {
            scope,
            outbox_id: outbox_id.into(),
            error_code: error_code.into(),
            retry_hint: retry_hint.into(),
            metadata: metadata.into(),
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn direct_secure_status(
        &self,
        scope: crate::internal::secure_direct::status::DirectSecureStatusScope,
        peer: crate::ids::PeerRef,
    ) -> crate::ImResult<crate::internal::secure_direct::status::DirectSecureLocalStatus> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::DirectSecureStatus { scope, peer, reply })
            .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn direct_secure_repair(
        &self,
        scope: crate::internal::secure_direct::status::DirectSecureStatusScope,
        peer: crate::ids::PeerRef,
    ) -> crate::ImResult<crate::internal::secure_direct::status::DirectSecureRepairPlan> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::DirectSecureRepair { scope, peer, reply })
            .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn prepare_direct_secure_prekeys(
        &self,
        input: crate::internal::secure_direct::prepare::DirectSecurePrekeyPrepareInput,
    ) -> crate::ImResult<crate::internal::secure_direct::prepare::DirectSecurePrekeyPrepareResult>
    {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::PrepareDirectSecurePrekeys { input, reply })
            .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn get_direct_secure_session(
        &self,
        owner_identity_id: impl Into<String>,
        peer_did: impl Into<String>,
    ) -> crate::ImResult<Option<crate::internal::secure_direct::sqlite_store::DirectSessionRecord>>
    {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::GetDirectSecureSession {
            owner_identity_id: owner_identity_id.into(),
            peer_did: peer_did.into(),
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn direct_init_session_material(
        &self,
        owner_identity_id: impl Into<String>,
        peer_did: impl Into<String>,
        session_id: impl Into<String>,
        signed_prekey_id: impl Into<String>,
        one_time_prekey_id: Option<String>,
    ) -> crate::ImResult<crate::internal::secure_direct::sqlite_store::DirectInitSessionMaterial>
    {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::DirectInitSessionMaterial {
            owner_identity_id: owner_identity_id.into(),
            peer_did: peer_did.into(),
            session_id: session_id.into(),
            signed_prekey_id: signed_prekey_id.into(),
            one_time_prekey_id,
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn save_incoming_direct_init_session(
        &self,
        commit: crate::internal::secure_direct::sqlite_store::DirectInitSessionCommit,
    ) -> crate::ImResult<crate::internal::secure_direct::sqlite_store::DirectInitSessionCommitResult>
    {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::SaveIncomingDirectInitSession { commit, reply })
            .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn save_outgoing_direct_init_session(
        &self,
        commit: crate::internal::secure_direct::sqlite_store::DirectInitSendCommit,
    ) -> crate::ImResult<crate::internal::secure_direct::sqlite_store::DirectInitSessionCommitResult>
    {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::SaveOutgoingDirectInitSession { commit, reply })
            .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn save_direct_secure_session_if_revision(
        &self,
        record: crate::internal::secure_direct::sqlite_store::DirectSessionRecord,
        expected_revision: i64,
    ) -> crate::ImResult<crate::internal::secure_direct::sqlite_store::DirectSessionCasResult> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::SaveDirectSecureSessionIfRevision {
            record,
            expected_revision,
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn retry_e2ee_outbox(
        &self,
        scope: crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope,
        outbox_id: impl Into<String>,
    ) -> crate::ImResult<Option<crate::internal::store::e2ee_outbox::E2eeOutboxRecord>> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::RetryE2eeOutbox {
            scope,
            outbox_id: outbox_id.into(),
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn drop_e2ee_outbox(
        &self,
        scope: crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope,
        outbox_id: impl Into<String>,
    ) -> crate::ImResult<Option<crate::internal::store::e2ee_outbox::E2eeOutboxRecord>> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::DropE2eeOutbox {
            scope,
            outbox_id: outbox_id.into(),
            reply,
        })
        .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn backfill_owner_identity_ids(
        &self,
        identities: Vec<super::schema::OwnerIdentityBackfill>,
    ) -> crate::ImResult<usize> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::BackfillOwnerIdentityIds { identities, reply })
            .await?;
        receiver.await.map_err(|_| actor_closed())?
    }

    pub(crate) async fn shutdown(&self) -> crate::ImResult<()> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::Shutdown { reply }).await?;
        receiver.await.map_err(|_| actor_closed())
    }

    async fn send(&self, command: LocalStateCommand) -> crate::ImResult<()> {
        self.sender.send(command).await.map_err(|_| actor_closed())
    }
}

fn run_actor(
    sqlite_path: PathBuf,
    mut receiver: mpsc::Receiver<LocalStateCommand>,
    ready_sender: oneshot::Sender<crate::ImResult<()>>,
) {
    let mut connection = match super::open_writable(&sqlite_path) {
        Ok(connection) => {
            let _ = ready_sender.send(Ok(()));
            connection
        }
        Err(err) => {
            let _ = ready_sender.send(Err(err));
            return;
        }
    };

    while let Some(command) = receiver.blocking_recv() {
        match command {
            LocalStateCommand::CurrentSchemaVersion { reply } => {
                let result = super::schema::current_schema_version(&connection);
                let _ = reply.send(result);
            }
            LocalStateCommand::EnsureConversation {
                owner_identity_id,
                owner_did,
                conversation_id,
                reply,
            } => {
                let result = super::conversation_registry::ensure_validated(
                    &connection,
                    &owner_identity_id,
                    &owner_did,
                    &conversation_id,
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::StoreMessages { records, reply } => {
                let result = super::messages::upsert_messages(&connection, &records);
                let _ = reply.send(result);
            }
            LocalStateCommand::StoreRemoteMessages {
                records,
                source_event_type,
                reply,
            } => {
                let result = (|| {
                    let transaction = connection
                        .transaction()
                        .map_err(super::local_state_unavailable)?;
                    let outcome = super::inbound_resolution_backlog::ingest_remote_messages(
                        &transaction,
                        &records,
                        &source_event_type,
                    )?;
                    transaction
                        .commit()
                        .map_err(super::local_state_unavailable)?;
                    Ok(outcome)
                })();
                let _ = reply.send(result);
            }
            LocalStateCommand::LoadGlobalCheckpoint {
                owner_identity_id,
                reply,
            } => {
                let result =
                    super::sync_state::load_global_checkpoint(&connection, &owner_identity_id);
                let _ = reply.send(result);
            }
            LocalStateCommand::ApplySyncDelta { input, reply } => {
                let result = apply_sync_delta(&mut connection, input);
                let _ = reply.send(result);
            }
            LocalStateCommand::ApplySystemNotification { input, reply } => {
                let result =
                    crate::internal::system_notification::store::apply(&mut connection, input);
                let _ = reply.send(result);
            }
            LocalStateCommand::ListSystemNotifications {
                owner_identity_id,
                owner_did,
                protocol_device_id,
                include_terminal,
                limit,
                reply,
            } => {
                let result = crate::internal::system_notification::store::list(
                    &connection,
                    &owner_identity_id,
                    &owner_did,
                    &protocol_device_id,
                    include_terminal,
                    limit,
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::GetSystemNotification {
                owner_identity_id,
                owner_did,
                protocol_device_id,
                event_id,
                reply,
            } => {
                let result = crate::internal::system_notification::store::get_by_event_id(
                    &connection,
                    &owner_identity_id,
                    &owner_did,
                    &protocol_device_id,
                    &event_id,
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::ListVerifiedSystemNotifications {
                owner_identity_id,
                owner_did,
                protocol_device_id,
                include_terminal,
                limit,
                reply,
            } => {
                let result = crate::internal::system_notification::store::list_verified(
                    &connection,
                    &owner_identity_id,
                    &owner_did,
                    &protocol_device_id,
                    include_terminal,
                    limit,
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::GetVerifiedSystemNotification {
                owner_identity_id,
                owner_did,
                protocol_device_id,
                join_session_id,
                reply,
            } => {
                let result = crate::internal::system_notification::store::get_verified_by_session(
                    &connection,
                    &owner_identity_id,
                    &owner_did,
                    &protocol_device_id,
                    &join_session_id,
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::UpsertContact { record, reply } => {
                let result = crate::internal::contact_store::records::upsert_contact(
                    &mut connection,
                    record,
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::UpsertDirectPeerRoute { record, reply } => {
                let result = super::direct_peer_routes::upsert(&connection, &record);
                let _ = reply.send(result);
            }
            LocalStateCommand::ProjectVerifiedHandle {
                owner_identity_id,
                owner_did,
                lookup,
                reply,
            } => {
                let result = super::peer_personas::project_verified_handle(
                    &mut connection,
                    &owner_identity_id,
                    &owner_did,
                    &lookup,
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::GetPersonaDisplayProfile {
                owner_identity_id,
                peer,
                reply,
            } => {
                let result = super::peer_profiles::display_profile_for_peer(
                    &connection,
                    &owner_identity_id,
                    &peer,
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::GetContactByDid {
                owner_identity_id,
                owner_did,
                did,
                reply,
            } => {
                let result = crate::internal::contact_store::records::get_contact_by_did(
                    &connection,
                    &owner_identity_id,
                    &owner_did,
                    &did,
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::GetCurrentContactByHandle {
                owner_identity_id,
                owner_did,
                handle,
                reply,
            } => {
                let result = crate::internal::contact_store::records::get_current_contact_by_handle(
                    &connection,
                    &owner_identity_id,
                    &owner_did,
                    &handle,
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::ListContacts {
                owner_identity_id,
                owner_did,
                limit,
                reply,
            } => {
                let result = crate::internal::contact_store::records::list_contacts(
                    &connection,
                    &owner_identity_id,
                    &owner_did,
                    limit,
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::ListContactDidsForMessageHistoryRecovery {
                owner_identity_id,
                owner_did,
                limit,
                reply,
            } => {
                let result =
                    crate::internal::contact_store::records::list_contact_dids_for_message_history_recovery(
                        &connection,
                        &owner_identity_id,
                        &owner_did,
                        limit,
                    );
                let _ = reply.send(result);
            }
            LocalStateCommand::ResolveContactHandleByDid {
                owner_identity_id,
                owner_did,
                did,
                reply,
            } => {
                let result =
                    crate::internal::contact_store::records::resolve_contact_handle_by_did_for_owner_identity(
                        &connection,
                        &owner_identity_id,
                        &owner_did,
                        &did,
                    );
                let _ = reply.send(result);
            }
            LocalStateCommand::ListDidsByHandle {
                owner_identity_id,
                owner_did,
                handle,
                reply,
            } => {
                let result =
                    crate::internal::contact_store::records::list_dids_by_handle_for_owner_identity(
                        &connection,
                        &owner_identity_id,
                        &owner_did,
                        &handle,
                    );
                let _ = reply.send(result);
            }
            LocalStateCommand::AppendRelationshipEvent { record, reply } => {
                let result = crate::internal::contact_store::records::append_relationship_event(
                    &connection,
                    record,
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::UpsertGroup { record, reply } => {
                let result = super::groups::upsert_group(&connection, record);
                let _ = reply.send(result);
            }
            LocalStateCommand::UpsertGroupE2eeSummary { summary, reply } => {
                let result = super::groups::upsert_group_e2ee_summary(&connection, summary);
                let _ = reply.send(result);
            }
            LocalStateCommand::ReplaceGroupMembers {
                owner_identity_id,
                owner_did,
                group_id,
                members,
                credential_name,
                reply,
            } => {
                let result = super::groups::replace_group_members(
                    &mut connection,
                    &owner_identity_id,
                    &owner_did,
                    &group_id,
                    &members,
                    &credential_name,
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::MarkGroupLeft {
                owner_identity_id,
                owner_did,
                group_id,
                group_did,
                credential_name,
                reply,
            } => {
                let result = super::groups::mark_group_left(
                    &mut connection,
                    &owner_identity_id,
                    &owner_did,
                    &group_id,
                    &group_did,
                    &credential_name,
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::GetGroupSnapshot {
                owner_identity_id,
                owner_did,
                group_id,
                reply,
            } => {
                let result = super::groups::get_group_snapshot_for_owner_identity(
                    &connection,
                    &owner_identity_id,
                    &owner_did,
                    &group_id,
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::ListCachedGroupMembers {
                owner_identity_id,
                owner_did,
                group_id,
                limit,
                reply,
            } => {
                let result = super::groups::list_cached_group_members_for_owner_identity(
                    &connection,
                    &owner_identity_id,
                    &owner_did,
                    &group_id,
                    limit,
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::ListGroupMessages {
                owner_identity_id,
                owner_did,
                group_id,
                limit,
                since_seq,
                reply,
            } => {
                let result = super::groups::list_group_messages_for_owner_identity(
                    &connection,
                    &owner_identity_id,
                    &owner_did,
                    &group_id,
                    limit,
                    since_seq,
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::ListDirectMessages {
                owner_identity_id,
                conversation_ids,
                limit,
                reply,
            } => {
                let result = super::messages::list_direct_messages_for_owner_identity(
                    &connection,
                    &owner_identity_id,
                    &conversation_ids,
                    limit,
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::ListDecryptedSecureMessages {
                owner_identity_id,
                message_ids,
                reply,
            } => {
                let result = super::messages::list_decrypted_secure_messages_for_owner_identity(
                    &connection,
                    &owner_identity_id,
                    &message_ids,
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::ListMessagesForThreadRef {
                owner_identity_id,
                owner_did,
                thread,
                limit,
                cursor,
                reply,
            } => {
                let result = super::messages::list_messages_for_thread_ref_for_owner_identity(
                    &connection,
                    &owner_identity_id,
                    &owner_did,
                    &thread,
                    limit,
                    cursor.as_deref(),
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::MaxServerSeqForThreadRef {
                owner_identity_id,
                owner_did,
                thread,
                reply,
            } => {
                let result = super::messages::max_server_seq_for_thread_ref_for_owner_identity(
                    &connection,
                    &owner_identity_id,
                    &owner_did,
                    &thread,
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::MessageServerSeq {
                owner_identity_id,
                message_id,
                reply,
            } => {
                let result = super::messages::message_server_seq(
                    &connection,
                    &owner_identity_id,
                    &message_id,
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::ListActiveGroupRefs {
                owner_identity_id,
                owner_did,
                limit,
                reply,
            } => {
                let result = super::groups::list_active_group_refs_for_owner_identity(
                    &connection,
                    &owner_identity_id,
                    &owner_did,
                    limit,
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::ClassifyMarkReadIds {
                owner_identity_id,
                owner_did,
                message_ids,
                reply,
            } => {
                let result = super::messages::classify_mark_read_ids_for_owner_identity(
                    &connection,
                    &owner_identity_id,
                    &owner_did,
                    &message_ids,
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::ListUnreadIncomingMessageIds {
                owner_identity_id,
                owner_did,
                thread,
                limit,
                reply,
            } => {
                let result = super::messages::list_unread_incoming_message_ids_for_owner_identity(
                    &connection,
                    &owner_identity_id,
                    &owner_did,
                    &thread,
                    limit,
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::ListIncomingMessageIdsUpToWatermark {
                owner_identity_id,
                owner_did,
                thread,
                read_watermark_message_id,
                read_watermark_seq,
                limit,
                reply,
            } => {
                let result =
                    super::messages::list_incoming_message_ids_up_to_watermark_for_owner_identity(
                        &connection,
                        &owner_identity_id,
                        &owner_did,
                        &thread,
                        read_watermark_message_id.as_deref(),
                        read_watermark_seq.as_deref(),
                        limit,
                    );
                let _ = reply.send(result);
            }
            LocalStateCommand::MarkMessagesRead {
                owner_identity_id,
                owner_did,
                message_ids,
                reply,
            } => {
                let result = super::messages::mark_messages_read_for_owner_identity(
                    &connection,
                    &owner_identity_id,
                    &owner_did,
                    &message_ids,
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::MarkThreadReadWatermark {
                owner_identity_id,
                owner_did,
                input,
                reply,
            } => {
                let result = super::messages::mark_thread_read_watermark_for_owner_identity(
                    &connection,
                    &owner_identity_id,
                    &owner_did,
                    input,
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::ListConversations {
                owner_identity_id,
                owner_did,
                query,
                reply,
            } => {
                let result = super::conversations::list_conversations_for_owner_identity(
                    &connection,
                    &owner_identity_id,
                    &owner_did,
                    &query,
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::ListMailNotifications {
                owner_identity_id,
                owner_did,
                limit,
                reply,
            } => {
                let result = super::email::list_mail_notifications_from_connection(
                    &connection,
                    &owner_identity_id,
                    &owner_did,
                    limit,
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::ListE2eeOutbox {
                scope,
                local_status,
                reply,
            } => {
                let result = crate::internal::store::e2ee_outbox::list_e2ee_outbox(
                    &connection,
                    &scope,
                    local_status.as_deref(),
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::QueueE2eeOutbox { record, reply } => {
                let result =
                    crate::internal::store::e2ee_outbox::queue_e2ee_outbox(&connection, record);
                let _ = reply.send(result);
            }
            LocalStateCommand::MarkE2eeOutboxSent {
                scope,
                outbox_id,
                session_id,
                sent_msg_id,
                sent_server_seq,
                metadata,
                reply,
            } => {
                let result = crate::internal::store::e2ee_outbox::mark_e2ee_outbox_sent(
                    &connection,
                    &scope,
                    &outbox_id,
                    &session_id,
                    &sent_msg_id,
                    sent_server_seq,
                    &metadata,
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::MarkE2eeOutboxSentAndStoreMessage {
                scope,
                outbox_id,
                session_id,
                sent_msg_id,
                sent_server_seq,
                metadata,
                message,
                reply,
            } => {
                let result = mark_e2ee_outbox_sent_and_store_message(
                    &mut connection,
                    &scope,
                    &outbox_id,
                    &session_id,
                    &sent_msg_id,
                    sent_server_seq,
                    &metadata,
                    &message,
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::SetE2eeOutboxFailure {
                scope,
                outbox_id,
                error_code,
                retry_hint,
                metadata,
                reply,
            } => {
                let result = crate::internal::store::e2ee_outbox::set_e2ee_outbox_failure_by_id(
                    &connection,
                    &scope,
                    &outbox_id,
                    &error_code,
                    &retry_hint,
                    &metadata,
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::DirectSecureStatus { scope, peer, reply } => {
                let result = crate::internal::secure_direct::status::direct_status_for_scope(
                    &connection,
                    &scope,
                    peer,
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::DirectSecureRepair { scope, peer, reply } => {
                let result = crate::internal::secure_direct::status::repair_direct_for_scope(
                    &connection,
                    &scope,
                    peer,
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::PrepareDirectSecurePrekeys { input, reply } => {
                let result =
                    crate::internal::secure_direct::prepare::prepare_direct_prekeys_for_connection(
                        &connection,
                        input,
                    );
                let _ = reply.send(result);
            }
            LocalStateCommand::GetDirectSecureSession {
                owner_identity_id,
                peer_did,
                reply,
            } => {
                let result =
                    crate::internal::secure_direct::sqlite_store::SqliteDirectSecureStateStore::new(
                        &connection,
                    )
                    .and_then(|store| store.get_session(&owner_identity_id, &peer_did));
                let _ = reply.send(result);
            }
            LocalStateCommand::DirectInitSessionMaterial {
                owner_identity_id,
                peer_did,
                session_id,
                signed_prekey_id,
                one_time_prekey_id,
                reply,
            } => {
                let result =
                    crate::internal::secure_direct::sqlite_store::SqliteDirectSecureStateStore::new(
                        &connection,
                    )
                    .and_then(|store| {
                        store.direct_init_session_material(
                            &owner_identity_id,
                            &peer_did,
                            &session_id,
                            &signed_prekey_id,
                            one_time_prekey_id.as_deref(),
                        )
                    });
                let _ = reply.send(result);
            }
            LocalStateCommand::SaveIncomingDirectInitSession { commit, reply } => {
                let result =
                    crate::internal::secure_direct::sqlite_store::save_incoming_init_session(
                        &mut connection,
                        commit,
                    );
                let _ = reply.send(result);
            }
            LocalStateCommand::SaveOutgoingDirectInitSession { commit, reply } => {
                let result =
                    crate::internal::secure_direct::sqlite_store::save_outgoing_init_session(
                        &mut connection,
                        commit,
                    );
                let _ = reply.send(result);
            }
            LocalStateCommand::SaveDirectSecureSessionIfRevision {
                record,
                expected_revision,
                reply,
            } => {
                let result =
                    crate::internal::secure_direct::sqlite_store::SqliteDirectSecureStateStore::new(
                        &connection,
                    )
                    .and_then(|store| store.save_session_if_revision(&record, expected_revision));
                let _ = reply.send(result);
            }
            LocalStateCommand::RetryE2eeOutbox {
                scope,
                outbox_id,
                reply,
            } => {
                let result = crate::internal::store::e2ee_outbox::retry_e2ee_outbox(
                    &connection,
                    &scope,
                    &outbox_id,
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::DropE2eeOutbox {
                scope,
                outbox_id,
                reply,
            } => {
                let result = crate::internal::store::e2ee_outbox::drop_e2ee_outbox(
                    &connection,
                    &scope,
                    &outbox_id,
                );
                let _ = reply.send(result);
            }
            LocalStateCommand::BackfillOwnerIdentityIds { identities, reply } => {
                let result = super::schema::backfill_owner_identity_ids(&connection, &identities);
                let _ = reply.send(result);
            }
            LocalStateCommand::Shutdown { reply } => {
                let _ = reply.send(());
                break;
            }
        }
    }
}

fn actor_closed() -> crate::ImError {
    crate::ImError::LocalStateUnavailable {
        detail: "local state actor is closed".to_string(),
    }
}

fn apply_sync_delta(
    connection: &mut rusqlite::Connection,
    input: super::sync_state::SyncDeltaApplyInput,
) -> crate::ImResult<super::sync_state::SyncDeltaApplyOutcome> {
    let transaction = connection
        .transaction()
        .map_err(super::local_state_unavailable)?;
    let result = super::sync_state::apply_sync_delta_tx(&transaction, input)?;
    transaction
        .commit()
        .map_err(super::local_state_unavailable)?;
    Ok(result)
}

fn mark_e2ee_outbox_sent_and_store_message(
    connection: &mut rusqlite::Connection,
    scope: &crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope,
    outbox_id: &str,
    session_id: &str,
    sent_msg_id: &str,
    sent_server_seq: Option<i64>,
    metadata: &str,
    message: &super::messages::MessageRecord,
) -> crate::ImResult<()> {
    let outbox_id = outbox_id.trim();
    if crate::internal::store::e2ee_outbox::get_e2ee_outbox(connection, scope, outbox_id)?.is_none()
    {
        return Err(crate::ImError::MessageNotFound {
            message_id: outbox_id.to_owned(),
        });
    }
    let transaction = connection
        .transaction()
        .map_err(super::local_state_unavailable)?;
    crate::internal::store::e2ee_outbox::mark_e2ee_outbox_sent(
        &transaction,
        scope,
        outbox_id,
        session_id,
        sent_msg_id,
        sent_server_seq,
        metadata,
    )?;
    super::messages::upsert_message(&transaction, message)?;
    transaction.commit().map_err(super::local_state_unavailable)
}

#[cfg(test)]
mod tests;
