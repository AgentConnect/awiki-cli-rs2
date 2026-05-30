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
    StoreMessages {
        records: Vec<super::messages::MessageRecord>,
        reply: oneshot::Sender<crate::ImResult<()>>,
    },
    UpsertContact {
        record: crate::internal::contact_store::records::ContactRecord,
        reply: oneshot::Sender<crate::ImResult<()>>,
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
    MarkMessagesRead {
        owner_identity_id: String,
        owner_did: String,
        message_ids: Vec<String>,
        reply: oneshot::Sender<crate::ImResult<i64>>,
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
    MergeRecoveredHandleLocalState {
        old_owner_dids: Vec<String>,
        new_owner_did: String,
        final_owner_identity_id: String,
        final_credential_name: String,
        reply: oneshot::Sender<
            crate::ImResult<(
                std::collections::BTreeMap<String, i64>,
                std::collections::BTreeMap<String, i64>,
            )>,
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

    pub(crate) async fn upsert_contact(
        &self,
        record: crate::internal::contact_store::records::ContactRecord,
    ) -> crate::ImResult<()> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::UpsertContact { record, reply })
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

    pub(crate) async fn merge_recovered_handle_local_state(
        &self,
        old_owner_dids: Vec<String>,
        new_owner_did: impl Into<String>,
        final_owner_identity_id: impl Into<String>,
        final_credential_name: impl Into<String>,
    ) -> crate::ImResult<(
        std::collections::BTreeMap<String, i64>,
        std::collections::BTreeMap<String, i64>,
    )> {
        let (reply, receiver) = oneshot::channel();
        self.send(LocalStateCommand::MergeRecoveredHandleLocalState {
            old_owner_dids,
            new_owner_did: new_owner_did.into(),
            final_owner_identity_id: final_owner_identity_id.into(),
            final_credential_name: final_credential_name.into(),
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
            LocalStateCommand::StoreMessages { records, reply } => {
                let result = super::messages::upsert_messages(&connection, &records);
                let _ = reply.send(result);
            }
            LocalStateCommand::UpsertContact { record, reply } => {
                let result = crate::internal::contact_store::records::upsert_contact(
                    &mut connection,
                    record,
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
            LocalStateCommand::MergeRecoveredHandleLocalState {
                old_owner_dids,
                new_owner_did,
                final_owner_identity_id,
                final_credential_name,
                reply,
            } => {
                let result =
                    crate::internal::identity_recover_local_state::merge_recovered_handle_local_state_for_connection(
                        &mut connection,
                        &old_owner_dids,
                        &new_owner_did,
                        &final_owner_identity_id,
                        &final_credential_name,
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
mod tests {
    use super::*;

    #[tokio::test]
    async fn db_actor_initializes_schema_and_returns_version() {
        let fixture = Fixture::new();
        let db = LocalStateDb::open(fixture.sqlite_path()).await.unwrap();

        let version = db.current_schema_version().await.unwrap();

        assert_eq!(version, super::super::schema::SCHEMA_VERSION);
        db.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn db_actor_backfill_owner_identity_ids_is_noop_for_v17_schema() {
        let fixture = Fixture::new();
        let db = LocalStateDb::open(fixture.sqlite_path()).await.unwrap();

        let updated = db
            .backfill_owner_identity_ids(vec![super::super::schema::OwnerIdentityBackfill {
                identity_id: "alice-id".to_string(),
                owner_did: "did:example:alice".to_string(),
                credential_names: vec!["alice".to_string()],
            }])
            .await
            .unwrap();

        assert_eq!(updated, 0);
        db.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn db_actor_stores_classifies_marks_and_lists_messages() {
        let fixture = Fixture::new();
        let db = LocalStateDb::open(fixture.sqlite_path()).await.unwrap();

        db.store_messages(vec![
            super::super::messages::MessageRecord {
                msg_id: "direct-1".to_string(),
                owner_identity_id: "alice-id".to_string(),
                owner_did: "did:example:alice".to_string(),
                conversation_id: "dm:did:example:bob".to_string(),
                thread_id: "dm:did:example:bob".to_string(),
                direction: 0,
                sender_did: "did:example:bob".to_string(),
                receiver_did: "did:example:alice".to_string(),
                content_type: "text/plain".to_string(),
                content: "hello".to_string(),
                sent_at: "2026-05-21T00:00:01Z".to_string(),
                stored_at: "2026-05-21T00:00:01Z".to_string(),
                is_read: false,
                credential_name: "alice".to_string(),
                ..super::super::messages::MessageRecord::default()
            },
            super::super::messages::MessageRecord {
                msg_id: "group-1".to_string(),
                owner_identity_id: "alice-id".to_string(),
                owner_did: "did:example:alice".to_string(),
                conversation_id: "group:did:example:group".to_string(),
                thread_id: "group:did:example:group".to_string(),
                direction: 0,
                sender_did: "did:example:bob".to_string(),
                group_id: "did:example:group".to_string(),
                group_did: "did:example:group".to_string(),
                content_type: "text/plain".to_string(),
                content: "group".to_string(),
                sent_at: "2026-05-21T00:00:02Z".to_string(),
                stored_at: "2026-05-21T00:00:02Z".to_string(),
                is_read: false,
                credential_name: "alice".to_string(),
                ..super::super::messages::MessageRecord::default()
            },
        ])
        .await
        .unwrap();

        let classification = db
            .classify_mark_read_ids(
                "alice-id",
                "did:example:alice",
                vec!["direct-1".to_string(), "group-1".to_string()],
            )
            .await
            .unwrap();
        assert_eq!(classification.direct_ids, vec!["direct-1"]);
        assert_eq!(classification.group_ids, vec!["group-1"]);

        let updated = db
            .mark_messages_read("alice-id", "did:example:alice", classification.local_ids())
            .await
            .unwrap();
        assert_eq!(updated, 2);

        let conversations = db
            .list_conversations(
                "alice-id",
                "did:example:alice",
                crate::messages::ConversationQuery {
                    limit: crate::ids::PageLimit(10),
                    include_groups: true,
                    include_direct: true,
                    unread_only: false,
                },
            )
            .await
            .unwrap();
        assert_eq!(conversations.len(), 2);
        assert!(conversations
            .iter()
            .any(|record| record.thread_id == "dm:did:example:bob"));
        assert!(conversations
            .iter()
            .any(|record| record.thread_id == "group:did:example:group"));
        db.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn db_actor_groups_projection_commands_use_existing_sql_helpers() {
        let fixture = Fixture::new();
        let db = LocalStateDb::open(fixture.sqlite_path()).await.unwrap();

        db.upsert_group(super::super::groups::GroupRecord {
            owner_identity_id: "alice-id".to_string(),
            owner_did: "did:example:alice".to_string(),
            group_id: "group-1".to_string(),
            group_did: "did:example:group:1".to_string(),
            name: "Async Group".to_string(),
            my_role: "owner".to_string(),
            membership_status: "active".to_string(),
            member_count: Some(2),
            last_message_at: "2026-05-21T00:00:03Z".to_string(),
            stored_at: "2026-05-21T00:00:00Z".to_string(),
            credential_name: "alice-id".to_string(),
            metadata: r#"{"message_security_profile":"group-e2ee"}"#.to_string(),
            ..super::super::groups::GroupRecord::default()
        })
        .await
        .unwrap();

        db.replace_group_members(
            "alice-id",
            "did:example:alice",
            "group-1",
            vec![
                super::super::groups::GroupMemberRecord {
                    owner_identity_id: "alice-id".to_string(),
                    owner_did: "did:example:alice".to_string(),
                    group_id: "group-1".to_string(),
                    user_id: "did:example:bob".to_string(),
                    member_did: "did:example:bob".to_string(),
                    member_handle: "bob.awiki.ai".to_string(),
                    role: "member".to_string(),
                    status: "active".to_string(),
                    joined_at: "2026-05-21T00:00:01Z".to_string(),
                    credential_name: "alice-id".to_string(),
                    ..super::super::groups::GroupMemberRecord::default()
                },
                super::super::groups::GroupMemberRecord {
                    owner_identity_id: "alice-id".to_string(),
                    owner_did: "did:example:alice".to_string(),
                    group_id: "group-1".to_string(),
                    user_id: "did:example:carol".to_string(),
                    member_did: "did:example:carol".to_string(),
                    member_handle: "carol.awiki.ai".to_string(),
                    role: "admin".to_string(),
                    status: "active".to_string(),
                    joined_at: "2026-05-21T00:00:02Z".to_string(),
                    credential_name: "alice-id".to_string(),
                    ..super::super::groups::GroupMemberRecord::default()
                },
            ],
            "alice-id",
        )
        .await
        .unwrap();

        db.store_messages(vec![super::super::messages::MessageRecord {
            msg_id: "group-msg-1".to_string(),
            owner_identity_id: "alice-id".to_string(),
            owner_did: "did:example:alice".to_string(),
            thread_id: "group:did:example:group:1".to_string(),
            direction: 0,
            sender_did: "did:example:bob".to_string(),
            group_id: "group-1".to_string(),
            group_did: "did:example:group:1".to_string(),
            content_type: "text/plain".to_string(),
            content: "hello group".to_string(),
            server_seq: Some(7),
            sent_at: "2026-05-21T00:00:04Z".to_string(),
            stored_at: "2026-05-21T00:00:04Z".to_string(),
            credential_name: "alice-id".to_string(),
            ..super::super::messages::MessageRecord::default()
        }])
        .await
        .unwrap();

        let snapshot = db
            .get_group_snapshot("alice-id", "did:example:alice", "did:example:group:1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot["name"], "Async Group");
        assert_eq!(snapshot["membership_status"], "active");

        let members = db
            .list_cached_group_members("alice-id", "did:example:alice", "did:example:group:1", 10)
            .await
            .unwrap();
        assert_eq!(members.len(), 2);
        assert_eq!(members[0]["role"], "admin");
        assert_eq!(members[1]["member_did"], "did:example:bob");

        let messages = db
            .list_group_messages(
                "alice-id",
                "did:example:alice",
                "did:example:group:1",
                10,
                Some(1),
            )
            .await
            .unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["msg_id"], "group-msg-1");

        let active_refs = db
            .list_active_group_refs("alice-id", "did:example:alice", 10)
            .await
            .unwrap();
        assert_eq!(active_refs, vec!["did:example:group:1"]);

        db.mark_group_left(
            "alice-id",
            "did:example:alice",
            "group-1",
            "did:example:group:1",
            "alice-id",
        )
        .await
        .unwrap();

        let active_refs = db
            .list_active_group_refs("alice-id", "did:example:alice", 10)
            .await
            .unwrap();
        assert!(active_refs.is_empty());
        let members = db
            .list_cached_group_members("alice-id", "did:example:alice", "did:example:group:1", 10)
            .await
            .unwrap();
        assert!(members.is_empty());

        db.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn db_actor_contact_commands_use_existing_store_helpers() {
        let fixture = Fixture::new();
        let db = LocalStateDb::open(fixture.sqlite_path()).await.unwrap();

        db.upsert_contact(crate::internal::contact_store::records::ContactRecord {
            owner_identity_id: "alice-id".to_string(),
            owner_did: "did:example:alice".to_string(),
            did: "did:example:bob".to_string(),
            handle: "bob.awiki.ai".to_string(),
            name: "Bob".to_string(),
            relationship: "following".to_string(),
            followed: Some(true),
            messaged: Some(true),
            last_seen_at: "2026-05-21T00:00:02Z".to_string(),
            source_type: "directory.test".to_string(),
            credential_name: "alice-id".to_string(),
            ..crate::internal::contact_store::records::ContactRecord::default()
        })
        .await
        .unwrap();
        db.upsert_contact(crate::internal::contact_store::records::ContactRecord {
            owner_identity_id: "alice-id".to_string(),
            owner_did: "did:example:alice".to_string(),
            did: "did:example:carol".to_string(),
            handle: "carol.awiki.ai".to_string(),
            name: "Carol".to_string(),
            relationship: "none".to_string(),
            followed: Some(false),
            messaged: Some(false),
            last_seen_at: "2026-05-21T00:00:01Z".to_string(),
            source_type: "directory.test".to_string(),
            credential_name: "alice-id".to_string(),
            ..crate::internal::contact_store::records::ContactRecord::default()
        })
        .await
        .unwrap();

        let by_did = db
            .get_contact_by_did("alice-id", "did:example:alice", "did:example:bob")
            .await
            .unwrap();
        assert_eq!(by_did.handle, "bob.awiki.ai");
        assert_eq!(by_did.followed, Some(true));

        let by_handle = db
            .get_current_contact_by_handle("alice-id", "did:example:alice", "carol.awiki.ai")
            .await
            .unwrap();
        assert_eq!(by_handle.did, "did:example:carol");

        let contacts = db
            .list_contacts("alice-id", "did:example:alice", 10)
            .await
            .unwrap();
        assert_eq!(contacts.len(), 2);
        assert_eq!(contacts[0].did, "did:example:bob");
        assert_eq!(contacts[1].did, "did:example:carol");

        let history_candidates = db
            .list_contact_dids_for_message_history_recovery("alice-id", "did:example:alice", 10)
            .await
            .unwrap();
        assert_eq!(
            history_candidates,
            vec!["did:example:bob", "did:example:carol"]
        );

        let handle = db
            .resolve_contact_handle_by_did("alice-id", "did:example:alice", "did:example:bob")
            .await
            .unwrap();
        assert_eq!(handle, "bob.awiki.ai");
        let dids = db
            .list_dids_by_handle("alice-id", "did:example:alice", "bob.awiki.ai")
            .await
            .unwrap();
        assert_eq!(dids, vec!["did:example:bob"]);

        let event_id = db
            .append_relationship_event(
                crate::internal::contact_store::records::RelationshipEventRecord {
                    owner_identity_id: "alice-id".to_string(),
                    owner_did: "did:example:alice".to_string(),
                    target_did: "did:example:bob".to_string(),
                    target_handle: "bob.awiki.ai".to_string(),
                    event_type: "followed".to_string(),
                    source_type: "directory.test".to_string(),
                    status: "applied".to_string(),
                    credential_name: "alice-id".to_string(),
                    ..crate::internal::contact_store::records::RelationshipEventRecord::default()
                },
            )
            .await
            .unwrap();
        assert!(!event_id.trim().is_empty());

        db.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn db_actor_merge_recovered_handle_local_state_uses_actor_connection() {
        let fixture = Fixture::new();
        let db = LocalStateDb::open(fixture.sqlite_path()).await.unwrap();
        let old_did = "did:example:old-alice";
        let new_did = "did:example:new-alice";

        db.store_messages(vec![super::super::messages::MessageRecord {
            msg_id: "recover-msg-1".to_string(),
            owner_identity_id: "old-alice".to_string(),
            owner_did: old_did.to_string(),
            conversation_id: "dm:did:example:bob".to_string(),
            thread_id: "dm:did:example:bob".to_string(),
            direction: 1,
            sender_did: old_did.to_string(),
            receiver_did: "did:example:bob".to_string(),
            content_type: "text/plain".to_string(),
            content: "pre recovery".to_string(),
            sent_at: "2026-05-21T00:00:01Z".to_string(),
            stored_at: "2026-05-21T00:00:01Z".to_string(),
            credential_name: "old-alice".to_string(),
            ..super::super::messages::MessageRecord::default()
        }])
        .await
        .unwrap();
        db.queue_e2ee_outbox(crate::internal::store::e2ee_outbox::E2eeOutboxRecord {
            outbox_id: "recover-outbox-1".to_string(),
            owner_identity_id: "old-alice".to_string(),
            owner_did: old_did.to_string(),
            credential_name: "old-alice".to_string(),
            peer_did: "did:example:bob".to_string(),
            plaintext: "stale secret".to_string(),
            local_status: "failed".to_string(),
            created_at: "2026-05-21T00:00:02Z".to_string(),
            updated_at: "2026-05-21T00:00:02Z".to_string(),
            ..crate::internal::store::e2ee_outbox::E2eeOutboxRecord::default()
        })
        .await
        .unwrap();

        let (store_merge, e2ee_cleanup) = db
            .merge_recovered_handle_local_state(
                vec![old_did.to_string()],
                new_did,
                "alice-recovered-id",
                "alice-recovered",
            )
            .await
            .unwrap();

        assert_eq!(store_merge.get("messages"), Some(&1));
        assert_eq!(e2ee_cleanup.get("e2ee_outbox"), Some(&1));
        let messages = db
            .list_conversations(
                "alice-recovered-id",
                new_did,
                crate::messages::ConversationQuery {
                    limit: crate::ids::PageLimit(10),
                    include_groups: false,
                    include_direct: true,
                    unread_only: false,
                },
            )
            .await
            .unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].thread_id, "dm:did:example:bob");
        let old_outbox = db
            .list_e2ee_outbox(
                crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope {
                    owner_identity_id: "old-alice".to_string(),
                    owner_did: old_did.to_string(),
                    credential_name: "old-alice".to_string(),
                },
                None,
            )
            .await
            .unwrap();
        assert!(old_outbox.is_empty());

        db.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn db_actor_e2ee_outbox_commands_use_existing_store_helpers() {
        let fixture = Fixture::new();
        let db = LocalStateDb::open(fixture.sqlite_path()).await.unwrap();
        let alice = crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope {
            owner_identity_id: "alice-id".to_string(),
            owner_did: "did:example:alice".to_string(),
            credential_name: "alice".to_string(),
        };
        let charlie = crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope {
            owner_identity_id: "charlie-id".to_string(),
            owner_did: "did:example:charlie".to_string(),
            credential_name: "charlie".to_string(),
        };
        {
            let connection = super::super::open_writable(&fixture.sqlite_path()).unwrap();
            crate::internal::store::e2ee_outbox::queue_e2ee_outbox(
                &connection,
                crate::internal::store::e2ee_outbox::E2eeOutboxRecord {
                    outbox_id: "outbox-1".to_string(),
                    owner_identity_id: alice.owner_identity_id.clone(),
                    owner_did: alice.owner_did.clone(),
                    credential_name: alice.credential_name.clone(),
                    peer_did: "did:example:bob".to_string(),
                    plaintext: "secret plaintext".to_string(),
                    local_status: "failed".to_string(),
                    created_at: "2026-05-24T00:00:00Z".to_string(),
                    updated_at: "2026-05-24T00:00:00Z".to_string(),
                    ..crate::internal::store::e2ee_outbox::E2eeOutboxRecord::default()
                },
            )
            .unwrap();
        }

        let failed = db
            .list_e2ee_outbox(alice.clone(), Some("failed".to_string()))
            .await
            .unwrap();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].outbox_id, "outbox-1");
        assert_eq!(failed[0].plaintext, "secret plaintext");

        assert!(db
            .retry_e2ee_outbox(charlie, "outbox-1")
            .await
            .unwrap()
            .is_none());
        let retried = db
            .retry_e2ee_outbox(alice.clone(), "outbox-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(retried.local_status, "queued");

        let dropped = db
            .drop_e2ee_outbox(alice, "outbox-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(dropped.local_status, "dropped");

        db.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn db_actor_e2ee_outbox_queue_uses_existing_store_helper() {
        let fixture = Fixture::new();
        let db = LocalStateDb::open(fixture.sqlite_path()).await.unwrap();
        let scope = crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope {
            owner_identity_id: "alice-id".to_string(),
            owner_did: "did:example:alice".to_string(),
            credential_name: "alice".to_string(),
        };

        let outbox_id = db
            .queue_e2ee_outbox(crate::internal::store::e2ee_outbox::E2eeOutboxRecord {
                outbox_id: "outbox-queued-by-actor".to_string(),
                owner_identity_id: scope.owner_identity_id.clone(),
                owner_did: scope.owner_did.clone(),
                credential_name: scope.credential_name.clone(),
                peer_did: "did:example:bob".to_string(),
                original_type: "markdown".to_string(),
                plaintext: "secret queued plaintext".to_string(),
                local_status: "queued".to_string(),
                last_error_code: "pending_confirmation".to_string(),
                retry_hint: "retry".to_string(),
                metadata: r#"{"source":"direct-send"}"#.to_string(),
                ..crate::internal::store::e2ee_outbox::E2eeOutboxRecord::default()
            })
            .await
            .unwrap();

        assert_eq!(outbox_id, "outbox-queued-by-actor");
        let queued = db
            .list_e2ee_outbox(scope, Some("queued".to_string()))
            .await
            .unwrap();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].outbox_id, "outbox-queued-by-actor");
        assert_eq!(queued[0].original_type, "markdown");
        assert_eq!(queued[0].plaintext, "secret queued plaintext");
        assert_eq!(queued[0].last_error_code, "pending_confirmation");
        assert_eq!(queued[0].retry_hint, "retry");
        assert_eq!(queued[0].metadata, r#"{"source":"direct-send"}"#);

        db.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn db_actor_e2ee_outbox_delivery_updates_are_owner_scoped() {
        let fixture = Fixture::new();
        let db = LocalStateDb::open(fixture.sqlite_path()).await.unwrap();
        let alice = crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope {
            owner_identity_id: "alice-id".to_string(),
            owner_did: "did:example:alice".to_string(),
            credential_name: "alice".to_string(),
        };
        let charlie = crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope {
            owner_identity_id: "charlie-id".to_string(),
            owner_did: "did:example:charlie".to_string(),
            credential_name: "charlie".to_string(),
        };
        {
            let connection = super::super::open_writable(&fixture.sqlite_path()).unwrap();
            crate::internal::store::e2ee_outbox::queue_e2ee_outbox(
                &connection,
                crate::internal::store::e2ee_outbox::E2eeOutboxRecord {
                    outbox_id: "outbox-delivery".to_string(),
                    owner_identity_id: alice.owner_identity_id.clone(),
                    owner_did: alice.owner_did.clone(),
                    credential_name: alice.credential_name.clone(),
                    peer_did: "did:example:bob".to_string(),
                    session_id: "old-session".to_string(),
                    plaintext: "secret plaintext".to_string(),
                    local_status: "queued".to_string(),
                    attempt_count: 2,
                    last_error_code: "previous_error".to_string(),
                    retry_hint: "retry".to_string(),
                    failed_msg_id: "failed-msg".to_string(),
                    failed_server_seq: Some(41),
                    metadata: "old-metadata".to_string(),
                    created_at: "2026-05-24T00:00:00Z".to_string(),
                    updated_at: "2026-05-24T00:00:00Z".to_string(),
                    ..crate::internal::store::e2ee_outbox::E2eeOutboxRecord::default()
                },
            )
            .unwrap();
        }

        db.mark_e2ee_outbox_sent(
            charlie.clone(),
            "outbox-delivery",
            "wrong-session",
            "wrong-msg",
            Some(99),
            "wrong-metadata",
        )
        .await
        .unwrap();
        let still_queued = db
            .list_e2ee_outbox(alice.clone(), Some("queued".to_string()))
            .await
            .unwrap();
        assert_eq!(still_queued.len(), 1);
        assert_eq!(still_queued[0].session_id, "old-session");
        assert_eq!(still_queued[0].attempt_count, 2);

        db.mark_e2ee_outbox_sent(
            alice.clone(),
            "outbox-delivery",
            "session-new",
            "msg-new",
            Some(42),
            "sent-metadata",
        )
        .await
        .unwrap();
        let sent = db
            .list_e2ee_outbox(alice.clone(), Some("sent".to_string()))
            .await
            .unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].local_status, "sent");
        assert_eq!(sent[0].attempt_count, 3);
        assert_eq!(sent[0].session_id, "session-new");
        assert_eq!(sent[0].sent_msg_id, "msg-new");
        assert_eq!(sent[0].sent_server_seq, Some(42));
        assert_eq!(sent[0].metadata, "sent-metadata");
        assert!(sent[0].last_error_code.is_empty());
        assert!(sent[0].retry_hint.is_empty());
        assert!(sent[0].failed_msg_id.is_empty());
        assert_eq!(sent[0].failed_server_seq, None);
        assert!(!sent[0].last_attempt_at.trim().is_empty());

        db.set_e2ee_outbox_failure(
            charlie,
            "outbox-delivery",
            "wrong_error",
            "drop",
            "wrong-metadata",
        )
        .await
        .unwrap();
        let still_sent = db
            .list_e2ee_outbox(alice.clone(), Some("sent".to_string()))
            .await
            .unwrap();
        assert_eq!(still_sent.len(), 1);
        assert!(still_sent[0].last_error_code.is_empty());

        db.set_e2ee_outbox_failure(
            alice.clone(),
            "outbox-delivery",
            "network_timeout",
            "retry",
            "failure-metadata",
        )
        .await
        .unwrap();
        let failed = db
            .list_e2ee_outbox(alice, Some("failed".to_string()))
            .await
            .unwrap();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].local_status, "failed");
        assert_eq!(failed[0].last_error_code, "network_timeout");
        assert_eq!(failed[0].retry_hint, "retry");
        assert_eq!(failed[0].metadata, "failure-metadata");
        assert_eq!(failed[0].attempt_count, 3);

        db.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn db_actor_e2ee_outbox_mark_sent_and_store_message_is_transactional() {
        let fixture = Fixture::new();
        let db = LocalStateDb::open(fixture.sqlite_path()).await.unwrap();
        let alice = crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope {
            owner_identity_id: "alice-id".to_string(),
            owner_did: "did:example:alice".to_string(),
            credential_name: "alice".to_string(),
        };
        let charlie = crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope {
            owner_identity_id: "charlie-id".to_string(),
            owner_did: "did:example:charlie".to_string(),
            credential_name: "charlie".to_string(),
        };
        {
            let connection = super::super::open_writable(&fixture.sqlite_path()).unwrap();
            crate::internal::store::e2ee_outbox::queue_e2ee_outbox(
                &connection,
                crate::internal::store::e2ee_outbox::E2eeOutboxRecord {
                    outbox_id: "outbox-tx".to_string(),
                    owner_identity_id: alice.owner_identity_id.clone(),
                    owner_did: alice.owner_did.clone(),
                    credential_name: alice.credential_name.clone(),
                    peer_did: "did:example:bob".to_string(),
                    plaintext: "secret plaintext".to_string(),
                    local_status: "queued".to_string(),
                    created_at: "2026-05-24T00:00:00Z".to_string(),
                    updated_at: "2026-05-24T00:00:00Z".to_string(),
                    ..crate::internal::store::e2ee_outbox::E2eeOutboxRecord::default()
                },
            )
            .unwrap();
        }
        let message = super::super::messages::MessageRecord {
            msg_id: "msg-outbox-tx".to_string(),
            owner_identity_id: alice.owner_identity_id.clone(),
            owner_did: alice.owner_did.clone(),
            conversation_id: "dm:did:example:bob".to_string(),
            thread_id: "dm:did:example:bob".to_string(),
            direction: 1,
            sender_did: alice.owner_did.clone(),
            receiver_did: "did:example:bob".to_string(),
            content_type: "text/plain".to_string(),
            content: "secret plaintext".to_string(),
            sent_at: "2026-05-24T00:00:01Z".to_string(),
            stored_at: "2026-05-24T00:00:01Z".to_string(),
            is_e2ee: true,
            is_read: true,
            metadata: "sent-metadata".to_string(),
            credential_name: alice.credential_name.clone(),
            ..super::super::messages::MessageRecord::default()
        };

        let wrong_owner = db
            .mark_e2ee_outbox_sent_and_store_message(
                charlie,
                "outbox-tx",
                "session-wrong",
                "msg-wrong",
                Some(99),
                "wrong-metadata",
                message.clone(),
            )
            .await;
        assert!(matches!(
            wrong_owner,
            Err(crate::ImError::MessageNotFound { message_id }) if message_id == "outbox-tx"
        ));
        let queued = db
            .list_e2ee_outbox(alice.clone(), Some("queued".to_string()))
            .await
            .unwrap();
        assert_eq!(queued.len(), 1);
        {
            let connection = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
            let count: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM messages WHERE msg_id = 'msg-outbox-tx'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 0);
        }

        db.mark_e2ee_outbox_sent_and_store_message(
            alice.clone(),
            "outbox-tx",
            "session-ok",
            "msg-outbox-tx",
            Some(7),
            "sent-metadata",
            message,
        )
        .await
        .unwrap();
        let sent = db
            .list_e2ee_outbox(alice.clone(), Some("sent".to_string()))
            .await
            .unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].session_id, "session-ok");
        assert_eq!(sent[0].sent_msg_id, "msg-outbox-tx");
        assert_eq!(sent[0].sent_server_seq, Some(7));
        assert_eq!(sent[0].metadata, "sent-metadata");
        {
            let connection = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
            let (content, is_e2ee): (String, i64) = connection
                .query_row(
                    "SELECT content, is_e2ee FROM messages WHERE msg_id = 'msg-outbox-tx'",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(content, "secret plaintext");
            assert_eq!(is_e2ee, 1);
        }

        db.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn db_actor_direct_secure_status_uses_existing_store_helpers() {
        let fixture = Fixture::new();
        let db = LocalStateDb::open(fixture.sqlite_path()).await.unwrap();
        let scope = crate::internal::secure_direct::status::DirectSecureStatusScope {
            owner_identity_id: "alice-id".to_string(),
            owner_did: "did:example:alice".to_string(),
        };
        {
            let connection = super::super::open_writable(&fixture.sqlite_path()).unwrap();
            let store =
                crate::internal::secure_direct::sqlite_store::SqliteDirectSecureStateStore::new(
                    &connection,
                )
                .unwrap();
            store
                .upsert_session(
                    &crate::internal::secure_direct::sqlite_store::DirectSessionRecord {
                        owner_identity_id: scope.owner_identity_id.clone(),
                        owner_did: scope.owner_did.clone(),
                        peer_did: "did:example:bob".to_string(),
                        session_id: "session-secret".to_string(),
                        state_blob: b"{}".to_vec(),
                        metadata_json: "{}".to_string(),
                        revision: 0,
                        created_at: "2026-05-24T00:00:00Z".to_string(),
                        updated_at: "2026-05-24T00:00:00Z".to_string(),
                    },
                )
                .unwrap();
        }

        let status = db
            .direct_secure_status(
                scope,
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(status.state, crate::secure::DirectSecureState::Ready);
        assert!(status.can_send_secure);
        assert_eq!(
            status.resolved_peer.as_ref().map(crate::ids::Did::as_str),
            Some("did:example:bob")
        );
        assert!(!format!("{status:?}").contains("session-secret"));
        db.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn db_actor_direct_secure_repair_removes_session_and_requeues_failed_outbox() {
        let fixture = Fixture::new();
        let db = LocalStateDb::open(fixture.sqlite_path()).await.unwrap();
        let scope = crate::internal::secure_direct::status::DirectSecureStatusScope {
            owner_identity_id: "alice-id".to_string(),
            owner_did: "did:example:alice".to_string(),
        };
        {
            let connection = super::super::open_writable(&fixture.sqlite_path()).unwrap();
            let store =
                crate::internal::secure_direct::sqlite_store::SqliteDirectSecureStateStore::new(
                    &connection,
                )
                .unwrap();
            store
                .upsert_session(
                    &crate::internal::secure_direct::sqlite_store::DirectSessionRecord {
                        owner_identity_id: scope.owner_identity_id.clone(),
                        owner_did: scope.owner_did.clone(),
                        peer_did: "did:example:bob".to_string(),
                        session_id: "session-to-repair".to_string(),
                        state_blob: b"secret-state".to_vec(),
                        metadata_json: "{}".to_string(),
                        revision: 0,
                        created_at: "2026-05-24T00:00:00Z".to_string(),
                        updated_at: "2026-05-24T00:00:00Z".to_string(),
                    },
                )
                .unwrap();
            crate::internal::store::e2ee_outbox::queue_e2ee_outbox(
                &connection,
                crate::internal::store::e2ee_outbox::E2eeOutboxRecord {
                    outbox_id: "outbox-direct-repair".to_string(),
                    owner_identity_id: scope.owner_identity_id.clone(),
                    owner_did: scope.owner_did.clone(),
                    credential_name: "alice".to_string(),
                    peer_did: "did:example:bob".to_string(),
                    original_type: "text".to_string(),
                    plaintext: "queued plaintext".to_string(),
                    local_status: "failed".to_string(),
                    last_error_code: "peer_error".to_string(),
                    retry_hint: "retry".to_string(),
                    ..Default::default()
                },
            )
            .unwrap();
        }

        let plan = db
            .direct_secure_repair(
                scope,
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            )
            .await
            .unwrap();

        assert!(plan.removed_session);
        assert_eq!(plan.requeued_outbox_count, 1);
        assert_eq!(
            plan.status.state,
            crate::secure::DirectSecureState::Preparing
        );
        assert!(!format!("{plan:?}").contains("secret-state"));
        {
            let connection = super::super::open_writable(&fixture.sqlite_path()).unwrap();
            let store =
                crate::internal::secure_direct::sqlite_store::SqliteDirectSecureStateStore::new(
                    &connection,
                )
                .unwrap();
            assert!(store
                .get_session("alice-id", "did:example:bob")
                .unwrap()
                .is_none());
            let status = connection
                .query_row(
                    "SELECT local_status FROM e2ee_outbox WHERE outbox_id = 'outbox-direct-repair'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap();
            assert_eq!(status, "queued");
        }
        db.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn db_actor_prepare_direct_secure_prekeys_persists_local_state_and_returns_publish_request(
    ) {
        let fixture = Fixture::new();
        let db = LocalStateDb::open(fixture.sqlite_path()).await.unwrap();
        let identity = TestIdentity::new("alice.actor-prekeys.example", "alice");

        let result = db
            .prepare_direct_secure_prekeys(
                crate::internal::secure_direct::prepare::DirectSecurePrekeyPrepareInput {
                    owner_identity_id: "alice-id".to_string(),
                    owner_did: identity.did.clone(),
                    identity_name: "alice".to_string(),
                    signing_key_id: format!("{}#key-1", identity.did),
                    agreement_key_id: format!("{}#key-3", identity.did),
                    signing_private_pem: identity.signing_private_pem,
                    agreement_private_pem: identity.agreement_private_pem,
                    local_did_document: identity.document,
                    peer: crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                },
            )
            .await
            .unwrap();

        assert_eq!(
            result.status.state,
            crate::secure::DirectSecureState::WaitingForPeer
        );
        assert_eq!(
            result.publish_request.method,
            "direct.e2ee.publish_prekey_bundle"
        );
        let params = serde_json::Value::Object(result.publish_request.params.clone());
        assert!(params.pointer("/body/prekey_bundle").is_some());
        assert_eq!(
            params
                .pointer("/body/one_time_prekeys")
                .and_then(serde_json::Value::as_array)
                .map(Vec::len),
            Some(16)
        );
        assert!(!format!("{result:?}").contains("PRIVATE KEY"));
        {
            let connection = super::super::open_writable(&fixture.sqlite_path()).unwrap();
            let store =
                crate::internal::secure_direct::sqlite_store::SqliteDirectSecureStateStore::new(
                    &connection,
                )
                .unwrap();
            assert!(store.active_signed_prekey("alice-id").unwrap().is_some());
            assert_eq!(
                store
                    .list_available_one_time_prekeys("alice-id")
                    .unwrap()
                    .len(),
                16
            );
        }
        db.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn db_actor_direct_secure_session_cas_rejects_stale_updates() {
        let fixture = Fixture::new();
        let db = LocalStateDb::open(fixture.sqlite_path()).await.unwrap();
        {
            let connection = super::super::open_writable(&fixture.sqlite_path()).unwrap();
            let store =
                crate::internal::secure_direct::sqlite_store::SqliteDirectSecureStateStore::new(
                    &connection,
                )
                .unwrap();
            store
                .upsert_session(
                    &crate::internal::secure_direct::sqlite_store::DirectSessionRecord {
                        owner_identity_id: "alice-id".to_string(),
                        owner_did: "did:example:alice".to_string(),
                        peer_did: "did:example:bob".to_string(),
                        session_id: "session-1".to_string(),
                        state_blob: b"state-v1".to_vec(),
                        metadata_json: "{}".to_string(),
                        revision: 0,
                        created_at: "2026-05-24T00:00:00Z".to_string(),
                        updated_at: "2026-05-24T00:00:00Z".to_string(),
                    },
                )
                .unwrap();
        }

        let loaded = db
            .get_direct_secure_session("alice-id", "did:example:bob")
            .await
            .unwrap()
            .unwrap();
        let mut first_update = loaded.clone();
        first_update.state_blob = b"state-v2".to_vec();
        first_update.updated_at = "2026-05-24T00:00:01Z".to_string();
        let saved = db
            .save_direct_secure_session_if_revision(first_update, loaded.revision)
            .await
            .unwrap();
        let crate::internal::secure_direct::sqlite_store::DirectSessionCasResult::Saved(saved) =
            saved
        else {
            panic!("first update should save");
        };
        assert_eq!(saved.revision, loaded.revision + 1);

        let mut stale_update = loaded;
        stale_update.state_blob = b"stale-state".to_vec();
        stale_update.updated_at = "2026-05-24T00:00:02Z".to_string();
        let stale_revision = stale_update.revision;
        let stale = db
            .save_direct_secure_session_if_revision(stale_update, stale_revision)
            .await
            .unwrap();
        let crate::internal::secure_direct::sqlite_store::DirectSessionCasResult::Stale {
            current,
            expected_revision,
        } = stale
        else {
            panic!("second update should be stale");
        };
        assert_eq!(expected_revision, 0);
        let current = current.unwrap();
        assert_eq!(current.revision, 1);
        assert_eq!(current.state_blob, b"state-v2");
        db.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn db_actor_incoming_direct_init_session_saves_session_and_consumes_opk_atomically() {
        let fixture = Fixture::new();
        let db = LocalStateDb::open(fixture.sqlite_path()).await.unwrap();
        {
            let connection = super::super::open_writable(&fixture.sqlite_path()).unwrap();
            let store =
                crate::internal::secure_direct::sqlite_store::SqliteDirectSecureStateStore::new(
                    &connection,
                )
                .unwrap();
            store
                .upsert_one_time_prekey(
                    &crate::internal::secure_direct::sqlite_store::DirectOneTimePrekeyRecord {
                        owner_identity_id: "alice-id".to_owned(),
                        owner_did: "did:example:alice".to_owned(),
                        key_id: "opk-init".to_owned(),
                        private_key_blob: b"private".to_vec(),
                        public_key_blob: b"public".to_vec(),
                        status:
                            crate::internal::secure_direct::sqlite_store::DirectPrekeyStatus::Available,
                        metadata_json: serde_json::json!({
                            "metadata": {
                                "key_id": "opk-init",
                                "public_key_b64u": "public",
                            }
                        })
                        .to_string(),
                        created_at: "2026-05-24T00:00:00Z".to_owned(),
                        consumed_at: String::new(),
                    },
                )
                .unwrap();
        }

        let saved = db
            .save_incoming_direct_init_session(
                crate::internal::secure_direct::sqlite_store::DirectInitSessionCommit {
                    record: crate::internal::secure_direct::sqlite_store::DirectSessionRecord {
                        owner_identity_id: "alice-id".to_owned(),
                        owner_did: "did:example:alice".to_owned(),
                        peer_did: "did:example:bob".to_owned(),
                        session_id: "session-init".to_owned(),
                        state_blob: b"state".to_vec(),
                        metadata_json: "{}".to_owned(),
                        revision: 0,
                        created_at: "2026-05-24T00:00:01Z".to_owned(),
                        updated_at: "2026-05-24T00:00:01Z".to_owned(),
                    },
                    expected_peer_revision: None,
                    consume_one_time_prekey_id: Some("opk-init".to_owned()),
                    consumed_at: "2026-05-24T00:00:02Z".to_owned(),
                },
            )
            .await
            .unwrap();

        assert!(matches!(
            saved,
            crate::internal::secure_direct::sqlite_store::DirectInitSessionCommitResult::Saved(_)
        ));
        {
            let connection = super::super::open_writable(&fixture.sqlite_path()).unwrap();
            let store =
                crate::internal::secure_direct::sqlite_store::SqliteDirectSecureStateStore::new(
                    &connection,
                )
                .unwrap();
            assert!(store
                .get_session("alice-id", "did:example:bob")
                .unwrap()
                .is_some());
            let opk = store
                .get_one_time_prekey("alice-id", "opk-init")
                .unwrap()
                .unwrap();
            assert_eq!(
                opk.status,
                crate::internal::secure_direct::sqlite_store::DirectPrekeyStatus::Consumed
            );
            assert_eq!(opk.consumed_at, "2026-05-24T00:00:02Z");
        }
        db.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn db_actor_shutdown_returns_closed_error_for_new_commands() {
        let fixture = Fixture::new();
        let db = LocalStateDb::open(fixture.sqlite_path()).await.unwrap();
        db.shutdown().await.unwrap();

        let err = db.current_schema_version().await.unwrap_err();

        assert_eq!(
            err,
            crate::ImError::LocalStateUnavailable {
                detail: "local state actor is closed".to_string(),
            }
        );
    }

    struct Fixture {
        root: tempfile::TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                root: tempfile::tempdir().unwrap(),
            }
        }

        fn sqlite_path(&self) -> PathBuf {
            self.root.path().join("local").join("im.sqlite")
        }
    }

    struct TestIdentity {
        did: String,
        document: serde_json::Value,
        signing_private_pem: String,
        agreement_private_pem: String,
    }

    impl TestIdentity {
        fn new(domain: &str, label: &str) -> Self {
            let service = anp::authentication::build_agent_message_service_with_options(
                "#message",
                format!("https://{domain}/anp-im/rpc"),
                anp::authentication::AnpMessageServiceOptions::default()
                    .with_service_did(format!("did:wba:{domain}")),
            );
            let bundle = anp::authentication::create_did_wba_document(
                domain,
                anp::authentication::DidDocumentOptions {
                    path_segments: vec!["agents".to_owned(), label.to_owned()],
                    domain: Some(domain.to_owned()),
                    challenge: Some(format!("direct-prekeys-{label}")),
                    services: vec![service],
                    did_profile: anp::authentication::DidProfile::E1,
                    ..Default::default()
                },
            )
            .unwrap();
            Self {
                did: bundle.did().unwrap().to_owned(),
                document: bundle.did_document.clone(),
                signing_private_pem: bundle.private_key_pem("key-1").unwrap().to_owned(),
                agreement_private_pem: bundle.private_key_pem("key-3").unwrap().to_owned(),
            }
        }
    }
}
