use crate::internal::auth::session::SessionProvider;
use crate::internal::transport::AuthenticatedRpcTransport;

pub struct ContentService<'a> {
    client: &'a crate::core::ImClient,
}

impl<'a> ContentService<'a> {
    pub(crate) fn new(client: &'a crate::core::ImClient) -> Self {
        Self { client }
    }

    pub fn create_page(&self, draft: super::PageDraft) -> crate::ImResult<super::PageDocument> {
        ContentRuntime::new(
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .create_page(draft)
    }

    pub fn list_pages(
        &self,
        query: super::ContentPageQuery,
    ) -> crate::ImResult<crate::ids::Page<super::PageDocument>> {
        ContentRuntime::new(
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .list_pages(query)
    }

    pub fn get_page(&self, page: super::PageRef) -> crate::ImResult<super::PageDocument> {
        ContentRuntime::new(
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .get_page(page)
    }

    pub fn update_page(
        &self,
        page: super::PageRef,
        patch: super::PageUpdate,
    ) -> crate::ImResult<super::PageDocument> {
        ContentRuntime::new(
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .update_page(page, patch)
    }

    pub fn rename_page(
        &self,
        page: super::PageRef,
        target: super::PageSlug,
    ) -> crate::ImResult<super::PageDocument> {
        ContentRuntime::new(
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .rename_page(page, target)
    }

    pub fn delete_page(&self, page: super::PageRef) -> crate::ImResult<super::PageDeleteResult> {
        ContentRuntime::new(
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .delete_page(page)
    }
}

pub(crate) struct ContentRuntime<P, T> {
    session_provider: P,
    transport: T,
}

impl<P, T> ContentRuntime<P, T>
where
    P: SessionProvider,
    T: AuthenticatedRpcTransport,
{
    pub(crate) fn new(session_provider: P, transport: T) -> Self {
        Self {
            session_provider,
            transport,
        }
    }

    pub(crate) fn create_page(
        mut self,
        draft: super::PageDraft,
    ) -> crate::ImResult<super::PageDocument> {
        self.ensure_session()?;
        let fallback_slug = draft.slug.clone();
        let call = super::wire::build_create_page_rpc_call(draft)?;
        let raw = self.call(call)?;
        super::wire::normalize_page(raw, Some(&fallback_slug))
    }

    pub(crate) fn list_pages(
        mut self,
        query: super::ContentPageQuery,
    ) -> crate::ImResult<crate::ids::Page<super::PageDocument>> {
        self.ensure_session()?;
        let call = super::wire::build_list_pages_rpc_call(query);
        let raw = self.call(call)?;
        super::wire::normalize_page_list(raw)
    }

    pub(crate) fn get_page(mut self, page: super::PageRef) -> crate::ImResult<super::PageDocument> {
        self.ensure_session()?;
        let fallback_slug = page.slug.clone();
        let call = super::wire::build_get_page_rpc_call(page);
        let raw = self.call(call)?;
        super::wire::normalize_page(raw, Some(&fallback_slug))
    }

    pub(crate) fn update_page(
        mut self,
        page: super::PageRef,
        patch: super::PageUpdate,
    ) -> crate::ImResult<super::PageDocument> {
        self.ensure_session()?;
        let fallback_slug = page.slug.clone();
        let call = super::wire::build_update_page_rpc_call(page, patch)?;
        let raw = self.call(call)?;
        super::wire::normalize_page(raw, Some(&fallback_slug))
    }

    pub(crate) fn rename_page(
        mut self,
        page: super::PageRef,
        target: super::PageSlug,
    ) -> crate::ImResult<super::PageDocument> {
        self.ensure_session()?;
        let fallback_slug = target.clone();
        let call = super::wire::build_rename_page_rpc_call(page, target);
        let raw = self.call(call)?;
        super::wire::normalize_page(raw, Some(&fallback_slug))
    }

    pub(crate) fn delete_page(
        mut self,
        page: super::PageRef,
    ) -> crate::ImResult<super::PageDeleteResult> {
        self.ensure_session()?;
        let call = super::wire::build_delete_page_rpc_call(page);
        let raw = self.call(call)?;
        Ok(super::wire::normalize_delete(raw))
    }

    fn ensure_session(&self) -> crate::ImResult<crate::auth::SessionBundle> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::Messaging)
    }

    fn call(&mut self, call: super::wire::RpcCall) -> crate::ImResult<serde_json::Value> {
        self.transport
            .authenticated_rpc(call.endpoint, call.method, call.params)
    }
}
