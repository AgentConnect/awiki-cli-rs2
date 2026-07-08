use crate::internal::auth::session::{AsyncSessionProvider, SessionProvider};
use crate::internal::transport::{AsyncAuthenticatedRpcTransport, AuthenticatedRpcTransport};

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

    pub async fn get_root_async(
        &self,
        domain: super::SiteDomain,
    ) -> crate::ImResult<super::SiteRootDocument> {
        SiteRuntime::new(
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .get_root_async(domain)
        .await
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

    pub async fn set_root_async(
        &self,
        draft: super::SiteRootDraft,
    ) -> crate::ImResult<super::SiteRootDocument> {
        SiteRuntime::new(
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .set_root_async(draft)
        .await
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

    pub async fn list_pages_async(
        &self,
        query: super::SitePageQuery,
    ) -> crate::ImResult<crate::ids::Page<super::SitePageDocument>> {
        SiteRuntime::new(
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .list_pages_async(query)
        .await
    }

    pub fn get_page(&self, page: super::SitePageRef) -> crate::ImResult<super::SitePageDocument> {
        SiteRuntime::new(
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .get_page(page)
    }

    pub async fn get_page_async(
        &self,
        page: super::SitePageRef,
    ) -> crate::ImResult<super::SitePageDocument> {
        SiteRuntime::new(
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .get_page_async(page)
        .await
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

    pub async fn create_page_async(
        &self,
        draft: super::SitePageDraft,
    ) -> crate::ImResult<super::SitePageDocument> {
        SiteRuntime::new(
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .create_page_async(draft)
        .await
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

    pub async fn update_page_async(
        &self,
        page: super::SitePageRef,
        patch: super::SitePageUpdate,
    ) -> crate::ImResult<super::SitePageDocument> {
        SiteRuntime::new(
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .update_page_async(page, patch)
        .await
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

    pub async fn rename_page_async(
        &self,
        page: super::SitePageRef,
        target: crate::content::PageSlug,
    ) -> crate::ImResult<super::SitePageDocument> {
        SiteRuntime::new(
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .rename_page_async(page, target)
        .await
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

    pub async fn delete_page_async(
        &self,
        page: super::SitePageRef,
    ) -> crate::ImResult<crate::content::PageDeleteResult> {
        SiteRuntime::new(
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .delete_page_async(page)
        .await
    }
}

pub(crate) struct SiteRuntime<P, T> {
    session_provider: P,
    transport: T,
}

impl<P, T> SiteRuntime<P, T> {
    pub(crate) fn new(session_provider: P, transport: T) -> Self {
        Self {
            session_provider,
            transport,
        }
    }
}

impl<P, T> SiteRuntime<P, T>
where
    P: SessionProvider,
    T: AuthenticatedRpcTransport,
{
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

impl<P, T> SiteRuntime<P, T>
where
    P: AsyncSessionProvider,
    T: AsyncAuthenticatedRpcTransport,
{
    pub(crate) async fn get_root_async(
        mut self,
        domain: super::SiteDomain,
    ) -> crate::ImResult<super::SiteRootDocument> {
        self.ensure_session_async().await?;
        let fallback_domain = domain.clone();
        let call = super::wire::build_get_root_rpc_call(domain);
        let raw = self.call_async(call).await?;
        super::wire::normalize_root(raw, &fallback_domain)
    }

    pub(crate) async fn set_root_async(
        mut self,
        draft: super::SiteRootDraft,
    ) -> crate::ImResult<super::SiteRootDocument> {
        self.ensure_session_async().await?;
        let fallback_domain = draft.domain.clone();
        let call = super::wire::build_set_root_rpc_call(draft);
        let raw = self.call_async(call).await?;
        super::wire::normalize_root(raw, &fallback_domain)
    }

    pub(crate) async fn list_pages_async(
        mut self,
        query: super::SitePageQuery,
    ) -> crate::ImResult<crate::ids::Page<super::SitePageDocument>> {
        self.ensure_session_async().await?;
        let fallback_domain = query.domain.clone();
        let call = super::wire::build_list_pages_rpc_call(query);
        let raw = self.call_async(call).await?;
        super::wire::normalize_page_list(&fallback_domain, raw)
    }

    pub(crate) async fn get_page_async(
        mut self,
        page: super::SitePageRef,
    ) -> crate::ImResult<super::SitePageDocument> {
        self.ensure_session_async().await?;
        let fallback_domain = page.domain.clone();
        let fallback_slug = page.slug.clone();
        let call = super::wire::build_get_page_rpc_call(page);
        let raw = self.call_async(call).await?;
        super::wire::normalize_page(raw, &fallback_domain, Some(&fallback_slug))
    }

    pub(crate) async fn create_page_async(
        mut self,
        draft: super::SitePageDraft,
    ) -> crate::ImResult<super::SitePageDocument> {
        self.ensure_session_async().await?;
        let fallback_domain = draft.domain.clone();
        let fallback_slug = draft.slug.clone();
        let call = super::wire::build_create_page_rpc_call(draft);
        let raw = self.call_async(call).await?;
        super::wire::normalize_page(raw, &fallback_domain, Some(&fallback_slug))
    }

    pub(crate) async fn update_page_async(
        mut self,
        page: super::SitePageRef,
        patch: super::SitePageUpdate,
    ) -> crate::ImResult<super::SitePageDocument> {
        self.ensure_session_async().await?;
        let fallback_domain = page.domain.clone();
        let fallback_slug = page.slug.clone();
        let call = super::wire::build_update_page_rpc_call(page, patch);
        let raw = self.call_async(call).await?;
        super::wire::normalize_page(raw, &fallback_domain, Some(&fallback_slug))
    }

    pub(crate) async fn rename_page_async(
        mut self,
        page: super::SitePageRef,
        target: crate::content::PageSlug,
    ) -> crate::ImResult<super::SitePageDocument> {
        self.ensure_session_async().await?;
        let fallback_domain = page.domain.clone();
        let fallback_slug = target.clone();
        let call = super::wire::build_rename_page_rpc_call(page, target);
        let raw = self.call_async(call).await?;
        super::wire::normalize_page(raw, &fallback_domain, Some(&fallback_slug))
    }

    pub(crate) async fn delete_page_async(
        mut self,
        page: super::SitePageRef,
    ) -> crate::ImResult<crate::content::PageDeleteResult> {
        self.ensure_session_async().await?;
        let call = super::wire::build_delete_page_rpc_call(page);
        let raw = self.call_async(call).await?;
        Ok(super::wire::normalize_delete(raw))
    }

    async fn ensure_session_async(&self) -> crate::ImResult<crate::auth::SessionBundle> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::Messaging)
            .await
    }

    async fn call_async(
        &mut self,
        call: super::wire::RpcCall,
    ) -> crate::ImResult<serde_json::Value> {
        self.transport
            .authenticated_rpc(call.endpoint, call.method, call.params)
            .await
    }
}
