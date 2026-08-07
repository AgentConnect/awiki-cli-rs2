use serde_json::{json, Value};

use super::{
    SiteDomain, SitePageDocument, SitePageDraft, SitePageQuery, SitePageRef, SiteRootDocument,
    SiteRootDraft,
};

pub(crate) const SITE_RPC_ENDPOINT: &str = "/user-service/v1/site/rpc";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransportProfile {
    RpcDefault,
    RpcReadHeavy,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RpcCall {
    pub(crate) endpoint: &'static str,
    pub(crate) method: &'static str,
    pub(crate) profile: TransportProfile,
    pub(crate) params: Value,
}

pub(crate) fn build_get_root_rpc_call(domain: SiteDomain) -> RpcCall {
    rpc_call(
        "get_root",
        TransportProfile::RpcReadHeavy,
        json!({ "domain": domain.as_str() }),
    )
}

pub(crate) fn build_set_root_rpc_call(draft: SiteRootDraft) -> RpcCall {
    rpc_call(
        "set_root",
        TransportProfile::RpcDefault,
        json!({ "domain": draft.domain.as_str(), "body": draft.body }),
    )
}

pub(crate) fn build_list_pages_rpc_call(query: SitePageQuery) -> RpcCall {
    rpc_call(
        "list_pages",
        TransportProfile::RpcReadHeavy,
        json!({ "domain": query.domain.as_str() }),
    )
}

pub(crate) fn build_get_page_rpc_call(page: SitePageRef) -> RpcCall {
    rpc_call(
        "get_page",
        TransportProfile::RpcReadHeavy,
        json!({ "domain": page.domain.as_str(), "slug": page.slug.as_str() }),
    )
}

pub(crate) fn build_create_page_rpc_call(draft: SitePageDraft) -> RpcCall {
    rpc_call(
        "create_page",
        TransportProfile::RpcDefault,
        json!({
            "domain": draft.domain.as_str(),
            "slug": draft.slug.as_str(),
            "body": draft.body,
        }),
    )
}

pub(crate) fn build_update_page_rpc_call(
    page: SitePageRef,
    patch: super::SitePageUpdate,
) -> RpcCall {
    rpc_call(
        "update_page",
        TransportProfile::RpcDefault,
        json!({
            "domain": page.domain.as_str(),
            "slug": page.slug.as_str(),
            "body": patch.body,
        }),
    )
}

pub(crate) fn build_rename_page_rpc_call(
    page: SitePageRef,
    target: crate::content::PageSlug,
) -> RpcCall {
    rpc_call(
        "rename_page",
        TransportProfile::RpcDefault,
        json!({
            "domain": page.domain.as_str(),
            "old_slug": page.slug.as_str(),
            "new_slug": target.as_str(),
        }),
    )
}

pub(crate) fn build_delete_page_rpc_call(page: SitePageRef) -> RpcCall {
    rpc_call(
        "delete_page",
        TransportProfile::RpcDefault,
        json!({ "domain": page.domain.as_str(), "slug": page.slug.as_str() }),
    )
}

pub(crate) fn normalize_root(
    value: Value,
    fallback_domain: &SiteDomain,
) -> crate::ImResult<SiteRootDocument> {
    let domain = string_field(&value, "domain")
        .map(SiteDomain::parse)
        .transpose()?
        .unwrap_or_else(|| fallback_domain.clone());
    Ok(SiteRootDocument {
        domain,
        body: string_field(&value, "body"),
        raw: value,
    })
}

pub(crate) fn normalize_page(
    value: Value,
    fallback_domain: &SiteDomain,
    fallback_slug: Option<&crate::content::PageSlug>,
) -> crate::ImResult<SitePageDocument> {
    let domain = string_field(&value, "domain")
        .map(SiteDomain::parse)
        .transpose()?
        .unwrap_or_else(|| fallback_domain.clone());
    let slug = string_field(&value, "slug")
        .or_else(|| fallback_slug.map(|slug| slug.as_str().to_string()))
        .ok_or_else(|| crate::ImError::Serialization {
            detail: "site page response is missing slug".to_string(),
        })?;
    Ok(SitePageDocument {
        domain,
        slug: crate::content::PageSlug::parse(slug)?,
        body: string_field(&value, "body"),
        raw: value,
    })
}

pub(crate) fn normalize_page_list(
    domain: &SiteDomain,
    value: Value,
) -> crate::ImResult<crate::ids::Page<SitePageDocument>> {
    let items = value
        .get("pages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|value| normalize_page(value, domain, None))
        .collect::<crate::ImResult<Vec<_>>>()?;
    let next_cursor = string_field(&value, "next_cursor")
        .or_else(|| string_field(&value, "nextCursor"))
        .map(crate::ids::Cursor::parse)
        .transpose()?;
    let has_more = value
        .get("has_more")
        .or_else(|| value.get("hasMore"))
        .and_then(Value::as_bool)
        .unwrap_or_else(|| next_cursor.is_some());
    Ok(crate::ids::Page {
        items,
        next_cursor,
        has_more,
    })
}

pub(crate) fn normalize_delete(value: Value) -> crate::content::PageDeleteResult {
    crate::content::PageDeleteResult {
        deleted: value
            .get("deleted")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        raw: value,
    }
}

fn rpc_call(method: &'static str, profile: TransportProfile, params: Value) -> RpcCall {
    RpcCall {
        endpoint: SITE_RPC_ENDPOINT,
        method,
        profile,
        params,
    }
}

fn string_field(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}
