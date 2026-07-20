use std::sync::{Arc, OnceLock};

#[derive(Clone)]
pub struct ImClient {
    core: Arc<super::ImCoreInner>,
    identity: crate::identity::IdentitySummary,
    runtime: Arc<crate::internal::identity_runtime::ClientIdentityRuntime>,
    conversation_store:
        Arc<OnceLock<Arc<crate::internal::runtime_store::conversation_store::ConversationStore>>>,
    message_store: Arc<OnceLock<Arc<crate::internal::runtime_store::message_store::MessageStore>>>,
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
        }
    }

    pub fn current_identity(&self) -> &crate::identity::IdentitySummary {
        &self.identity
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
}
