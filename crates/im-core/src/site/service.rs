use crate::internal::auth::session::SessionProvider;
use crate::internal::transport::AuthenticatedRpcTransport;

pub struct SiteService<'a> {
    client: &'a crate::core::ImClient,
}

impl<'a> SiteService<'a> {
    pub(crate) fn new(client: &'a crate::core::ImClient) -> Self {
        Self { client }
    }

    pub fn get_root(&self, domain: super::SiteDomain) -> crate::ImResult<super::SiteRootDocument> {
        SiteRuntime::new(
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .get_root(domain)
    }

    pub fn set_root(
        &self,
        draft: super::SiteRootDraft,
    ) -> crate::ImResult<super::SiteRootDocument> {
        SiteRuntime::new(
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .set_root(draft)
    }

    pub fn list_pages(
        &self,
        query: super::SitePageQuery,
    ) -> crate::ImResult<crate::ids::Page<super::SitePageDocument>> {
        SiteRuntime::new(
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .list_pages(query)
    }

    pub fn get_page(&self, page: super::SitePageRef) -> crate::ImResult<super::SitePageDocument> {
        SiteRuntime::new(
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .get_page(page)
    }

    pub fn create_page(
        &self,
        draft: super::SitePageDraft,
    ) -> crate::ImResult<super::SitePageDocument> {
        SiteRuntime::new(
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .create_page(draft)
    }

    pub fn update_page(
        &self,
        page: super::SitePageRef,
        patch: super::SitePageUpdate,
    ) -> crate::ImResult<super::SitePageDocument> {
        SiteRuntime::new(
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .update_page(page, patch)
    }

    pub fn rename_page(
        &self,
        page: super::SitePageRef,
        target: crate::content::PageSlug,
    ) -> crate::ImResult<super::SitePageDocument> {
        SiteRuntime::new(
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .rename_page(page, target)
    }

    pub fn delete_page(
        &self,
        page: super::SitePageRef,
    ) -> crate::ImResult<crate::content::PageDeleteResult> {
        SiteRuntime::new(
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .delete_page(page)
    }
}

pub(crate) struct SiteRuntime<P, T> {
    session_provider: P,
    transport: T,
}

impl<P, T> SiteRuntime<P, T>
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

    pub(crate) fn get_root(
        mut self,
        domain: super::SiteDomain,
    ) -> crate::ImResult<super::SiteRootDocument> {
        self.ensure_session()?;
        let fallback_domain = domain.clone();
        let call = super::wire::build_get_root_rpc_call(domain);
        let raw = self.call(call)?;
        super::wire::normalize_root(raw, &fallback_domain)
    }

    pub(crate) fn set_root(
        mut self,
        draft: super::SiteRootDraft,
    ) -> crate::ImResult<super::SiteRootDocument> {
        self.ensure_session()?;
        let fallback_domain = draft.domain.clone();
        let call = super::wire::build_set_root_rpc_call(draft);
        let raw = self.call(call)?;
        super::wire::normalize_root(raw, &fallback_domain)
    }

    pub(crate) fn list_pages(
        mut self,
        query: super::SitePageQuery,
    ) -> crate::ImResult<crate::ids::Page<super::SitePageDocument>> {
        self.ensure_session()?;
        let fallback_domain = query.domain.clone();
        let call = super::wire::build_list_pages_rpc_call(query);
        let raw = self.call(call)?;
        super::wire::normalize_page_list(&fallback_domain, raw)
    }

    pub(crate) fn get_page(
        mut self,
        page: super::SitePageRef,
    ) -> crate::ImResult<super::SitePageDocument> {
        self.ensure_session()?;
        let fallback_domain = page.domain.clone();
        let fallback_slug = page.slug.clone();
        let call = super::wire::build_get_page_rpc_call(page);
        let raw = self.call(call)?;
        super::wire::normalize_page(raw, &fallback_domain, Some(&fallback_slug))
    }

    pub(crate) fn create_page(
        mut self,
        draft: super::SitePageDraft,
    ) -> crate::ImResult<super::SitePageDocument> {
        self.ensure_session()?;
        let fallback_domain = draft.domain.clone();
        let fallback_slug = draft.slug.clone();
        let call = super::wire::build_create_page_rpc_call(draft);
        let raw = self.call(call)?;
        super::wire::normalize_page(raw, &fallback_domain, Some(&fallback_slug))
    }

    pub(crate) fn update_page(
        mut self,
        page: super::SitePageRef,
        patch: super::SitePageUpdate,
    ) -> crate::ImResult<super::SitePageDocument> {
        self.ensure_session()?;
        let fallback_domain = page.domain.clone();
        let fallback_slug = page.slug.clone();
        let call = super::wire::build_update_page_rpc_call(page, patch);
        let raw = self.call(call)?;
        super::wire::normalize_page(raw, &fallback_domain, Some(&fallback_slug))
    }

    pub(crate) fn rename_page(
        mut self,
        page: super::SitePageRef,
        target: crate::content::PageSlug,
    ) -> crate::ImResult<super::SitePageDocument> {
        self.ensure_session()?;
        let fallback_domain = page.domain.clone();
        let fallback_slug = target.clone();
        let call = super::wire::build_rename_page_rpc_call(page, target);
        let raw = self.call(call)?;
        super::wire::normalize_page(raw, &fallback_domain, Some(&fallback_slug))
    }

    pub(crate) fn delete_page(
        mut self,
        page: super::SitePageRef,
    ) -> crate::ImResult<crate::content::PageDeleteResult> {
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
