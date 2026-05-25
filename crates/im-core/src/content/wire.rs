use serde_json::{json, Map, Value};

use super::{ContentPageQuery, PageDeleteResult, PageDocument, PageDraft, PageRef, PageSlug};

pub(crate) const CONTENT_RPC_ENDPOINT: &str = "/content/rpc";

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

pub(crate) fn build_create_page_rpc_call(draft: PageDraft) -> crate::ImResult<RpcCall> {
    if draft.title.trim().is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("title".to_string()),
            "title is required",
        ));
    }
    Ok(rpc_call(
        "create",
        TransportProfile::RpcDefault,
        json!({
            "slug": draft.slug.as_str(),
            "title": draft.title.trim(),
            "body": draft.body,
            "visibility": draft.visibility.as_str(),
        }),
    ))
}

pub(crate) fn build_list_pages_rpc_call(_query: ContentPageQuery) -> RpcCall {
    rpc_call("list", TransportProfile::RpcReadHeavy, json!({}))
}

pub(crate) fn build_get_page_rpc_call(page: PageRef) -> RpcCall {
    rpc_call(
        "get",
        TransportProfile::RpcReadHeavy,
        json!({ "slug": page.slug.as_str() }),
    )
}

pub(crate) fn build_update_page_rpc_call(
    page: PageRef,
    patch: super::PageUpdate,
) -> crate::ImResult<RpcCall> {
    let mut payload = Map::new();
    payload.insert(
        "slug".to_string(),
        Value::String(page.slug.as_str().to_string()),
    );
    if let Some(title) = patch.title.filter(|value| !value.trim().is_empty()) {
        payload.insert("title".to_string(), Value::String(title.trim().to_string()));
    }
    if let Some(body) = patch.body {
        payload.insert("body".to_string(), Value::String(body));
    }
    if let Some(visibility) = patch.visibility {
        payload.insert(
            "visibility".to_string(),
            Value::String(visibility.as_str().to_string()),
        );
    }
    if payload.len() == 1 {
        return Err(crate::ImError::invalid_input(
            None,
            "no update fields were provided",
        ));
    }
    Ok(rpc_call(
        "update",
        TransportProfile::RpcDefault,
        Value::Object(payload),
    ))
}

pub(crate) fn build_rename_page_rpc_call(page: PageRef, target: PageSlug) -> RpcCall {
    rpc_call(
        "rename",
        TransportProfile::RpcDefault,
        json!({
            "old_slug": page.slug.as_str(),
            "new_slug": target.as_str(),
        }),
    )
}

pub(crate) fn build_delete_page_rpc_call(page: PageRef) -> RpcCall {
    rpc_call(
        "delete",
        TransportProfile::RpcDefault,
        json!({ "slug": page.slug.as_str() }),
    )
}

pub(crate) fn normalize_page(
    value: Value,
    fallback_slug: Option<&PageSlug>,
) -> crate::ImResult<PageDocument> {
    let slug = string_field(&value, "slug")
        .or_else(|| fallback_slug.map(|slug| slug.as_str().to_string()))
        .ok_or_else(|| crate::ImError::Serialization {
            detail: "content page response is missing slug".to_string(),
        })?;
    Ok(PageDocument {
        slug: PageSlug::parse(slug)?,
        title: string_field(&value, "title"),
        body: string_field(&value, "body"),
        visibility: string_field(&value, "visibility")
            .map(super::Visibility::parse)
            .transpose()?,
        raw: value,
    })
}

pub(crate) fn normalize_page_list(value: Value) -> crate::ImResult<crate::ids::Page<PageDocument>> {
    let items = value
        .get("pages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|value| normalize_page(value, None))
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

pub(crate) fn normalize_delete(value: Value) -> PageDeleteResult {
    PageDeleteResult {
        deleted: value
            .get("deleted")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        raw: value,
    }
}

fn rpc_call(method: &'static str, profile: TransportProfile, params: Value) -> RpcCall {
    RpcCall {
        endpoint: CONTENT_RPC_ENDPOINT,
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
