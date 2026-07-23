use std::sync::{Arc, OnceLock};

#[derive(Clone)]
pub struct ImClient {
    core: Arc<super::ImCoreInner>,
    identity: crate::identity::IdentitySummary,
    runtime: Arc<crate::internal::identity_runtime::ClientIdentityRuntime>,
    conversation_store:
        Arc<OnceLock<Arc<crate::internal::runtime_store::conversation_store::ConversationStore>>>,
    message_store: Arc<OnceLock<Arc<crate::internal::runtime_store::message_store::MessageStore>>>,
    system_notification_store: Arc<
        OnceLock<
            Arc<crate::internal::runtime_store::system_notification_store::SystemNotificationStore>,
        >,
    >,
}

impl ImClient {
    pub(crate) fn new(
        core: Arc<super::ImCoreInner>,
        runtime: crate::internal::identity_runtime::ClientIdentityRuntime,
    ) -> Self {
        let identity = runtime.summary.clone();
        Self {
            core,
            identity,
            runtime: Arc::new(runtime),
            conversation_store: Arc::new(OnceLock::new()),
            message_store: Arc::new(OnceLock::new()),
            system_notification_store: Arc::new(OnceLock::new()),
        }
    }

    pub fn current_identity(&self) -> &crate::identity::IdentitySummary {
        &self.identity
    }

    pub(crate) fn exact_protocol_device_id(&self) -> crate::ImResult<String> {
        let summary = self.core_handle().identities().device_summary(
            crate::identity::IdentitySelector::Id(self.current_identity().id.clone()),
        )?;
        if summary.mode != crate::identity::IdentityDeviceMode::VNext
            || summary.readiness == crate::identity::IdentityDeviceReadiness::Blocked
        {
            return Err(crate::ImError::PermissionDenied);
        }
        summary
            .protocol_device_id
            .map(|device_id| device_id.as_str().to_owned())
            .ok_or(crate::ImError::PermissionDenied)
    }

    pub fn did(&self) -> &crate::ids::Did {
        &self.identity.did
    }

    pub fn handle(&self) -> Option<&crate::ids::Handle> {
        self.identity.handle.as_ref()
    }

    pub fn did_domain(&self) -> &str {
        self.core.sdk_config().did_domain.as_str()
    }

    pub fn auth(&self) -> crate::auth::AuthService<'_> {
        crate::auth::AuthService::new(self)
    }

    pub fn identity(&self) -> crate::identity::IdentityService<'_> {
        crate::identity::IdentityService::new(self)
    }

    pub fn directory(&self) -> crate::directory::DirectoryService<'_> {
        crate::directory::DirectoryService::new(self)
    }

    pub fn content(&self) -> crate::content::ContentService<'_> {
        crate::content::ContentService::new(self)
    }

    pub fn site(&self) -> crate::site::SiteService<'_> {
        crate::site::SiteService::new(self)
    }

    pub fn messages(&self) -> crate::messages::MessageService<'_> {
        crate::messages::MessageService::new(self)
    }

    pub fn attachments(&self) -> crate::attachments::AttachmentService<'_> {
        crate::attachments::AttachmentService::new(self)
    }

    pub fn groups(&self) -> crate::groups::GroupService<'_> {
        crate::groups::GroupService::new(self)
    }

    pub fn realtime(&self) -> crate::realtime::RealtimeService<'_> {
        crate::realtime::RealtimeService::new(self)
    }

    pub fn email(&self) -> crate::email::EmailService<'_> {
        crate::email::EmailService::new(self)
    }

    pub fn secure(&self) -> crate::secure::SecureService<'_> {
        crate::secure::SecureService::new(self)
    }

    pub fn system_notifications(
        &self,
    ) -> crate::system_notifications::SystemNotificationService<'_> {
        crate::system_notifications::SystemNotificationService::new(self)
    }

    pub fn root_key_transfer(&self) -> crate::identity::RootKeyTransferService<'_> {
        crate::identity::RootKeyTransferService::new(self)
    }

    pub(crate) fn runtime(&self) -> &crate::internal::identity_runtime::ClientIdentityRuntime {
        &self.runtime
    }

    pub(crate) fn core_inner(&self) -> &super::ImCoreInner {
        &self.core
    }

    pub(crate) fn core_handle(&self) -> super::ImCore {
        super::ImCore {
            inner: self.core.clone(),
        }
    }

    pub(crate) fn conversation_store(
        &self,
    ) -> Arc<crate::internal::runtime_store::conversation_store::ConversationStore> {
        self.conversation_store
            .get_or_init(|| {
                crate::internal::runtime_store::conversation_store::ConversationStore::new_for_client(
                    self,
                )
            })
            .clone()
    }

    pub(crate) fn emit_committed_conversation_projection(&self, reason: &str) {
        let Some(store) = self.conversation_store.get() else {
            return;
        };
        store.on_committed_local_projection(self, reason);
    }

    pub(crate) fn emit_committed_local_message_projection(&self, reason: &str) {
        self.emit_committed_conversation_projection(reason);
        self.emit_committed_message_projection(reason);
    }

    pub(crate) fn message_store(
        &self,
    ) -> Arc<crate::internal::runtime_store::message_store::MessageStore> {
        self.message_store
            .get_or_init(|| {
                crate::internal::runtime_store::message_store::MessageStore::new_for_client(self)
            })
            .clone()
    }

    pub(crate) fn emit_committed_message_projection(&self, reason: &str) {
        let Some(store) = self.message_store.get() else {
            return;
        };
        store.on_committed_local_projection(self, reason);
    }

    pub(crate) fn emit_committed_message_sync_invalidation_if_initialized(
        &self,
        invalidation: &crate::internal::local_state::sync_state::SyncDeltaInvalidation,
    ) {
        let Some(store) = self.message_store.get() else {
            return;
        };
        store.on_committed_sync_invalidation(self, invalidation);
    }

    pub(crate) fn system_notification_store(
        &self,
    ) -> Arc<crate::internal::runtime_store::system_notification_store::SystemNotificationStore>
    {
        self.system_notification_store
            .get_or_init(|| {
                crate::internal::runtime_store::system_notification_store::SystemNotificationStore::new_for_client(self)
            })
            .clone()
    }

    pub(crate) fn emit_committed_system_notification(
        &self,
        item: crate::system_notifications::SystemNotificationSnapshot,
    ) {
        let Some(store) = self.system_notification_store.get() else {
            return;
        };
        store.emit_committed(self, item);
    }

    pub(crate) async fn list_verified_device_join_notifications(
        &self,
        include_terminal: bool,
    ) -> crate::ImResult<Vec<crate::internal::system_notification::wire::JoinNotification>> {
        #[cfg(feature = "sqlite")]
        {
            let protocol_device_id = self.exact_protocol_device_id().map_err(|_| {
                crate::ImError::invalid_input(
                    Some("identity.protocol_device_id".to_owned()),
                    "verified Join notifications require an exact-device identity",
                )
            })?;
            self.core_inner()
                .local_state_db()
                .await?
                .list_verified_system_notifications(
                    self.current_identity().id.as_str(),
                    self.did().as_str(),
                    protocol_device_id,
                    include_terminal,
                    500,
                )
                .await
        }
        #[cfg(not(feature = "sqlite"))]
        {
            let _ = include_terminal;
            Err(crate::ImError::unsupported(
                "system-notification-local-state",
            ))
        }
    }

    pub(crate) async fn get_verified_device_join_notification(
        &self,
        join_session_id: &str,
    ) -> crate::ImResult<Option<crate::internal::system_notification::wire::JoinNotification>> {
        #[cfg(feature = "sqlite")]
        {
            let protocol_device_id = self.exact_protocol_device_id().map_err(|_| {
                crate::ImError::invalid_input(
                    Some("identity.protocol_device_id".to_owned()),
                    "verified Join notifications require an exact-device identity",
                )
            })?;
            self.core_inner()
                .local_state_db()
                .await?
                .get_verified_system_notification(
                    self.current_identity().id.as_str(),
                    self.did().as_str(),
                    protocol_device_id,
                    join_session_id,
                )
                .await
        }
        #[cfg(not(feature = "sqlite"))]
        {
            let _ = join_session_id;
            Err(crate::ImError::unsupported(
                "system-notification-local-state",
            ))
        }
    }
}
