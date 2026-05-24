use std::sync::Arc;

#[derive(Clone)]
pub struct ImClient {
    core: Arc<super::ImCoreInner>,
    identity: crate::identity::IdentitySummary,
    runtime: Arc<crate::internal::identity_runtime::ClientIdentityRuntime>,
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

    pub fn auth(&self) -> crate::auth::AuthService<'_> {
        crate::auth::AuthService::new(self)
    }

    pub fn identity(&self) -> crate::identity::IdentityService<'_> {
        crate::identity::IdentityService::new(self)
    }

    pub fn directory(&self) -> crate::directory::DirectoryService<'_> {
        crate::directory::DirectoryService::new(self)
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
}
