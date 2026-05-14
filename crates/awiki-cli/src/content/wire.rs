use super::types::{
    CommandResult, ContentError, CreatePageParams, IdentitySummary, RenamePageParams,
    UpdatePageParams, CONTENT_RPC_ENDPOINT,
};
use crate::transportcfg::Profile;
use serde_json::{json, Map, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct RpcCall {
    pub endpoint: &'static str,
    pub method: &'static str,
    pub profile: Profile,
    pub params: Value,
}

pub fn build_create_page_rpc_call(params: CreatePageParams) -> Result<RpcCall, ContentError> {
    let slug = params.slug.trim();
    let title = params.title.trim();
    if slug.is_empty() {
        return Err(ContentError::SlugRequired);
    }
    if title.is_empty() {
        return Err(ContentError::TitleRequired);
    }
    let visibility = normalize_visibility(&params.visibility, false)?;
    let mut payload = json!({
        "slug": slug,
        "title": title,
        "body": params.body,
    });
    if !visibility.is_empty() {
        payload["visibility"] = Value::String(visibility);
    }
    Ok(rpc_call("create", Profile::RpcDefault, payload))
}

pub fn build_list_pages_rpc_call() -> RpcCall {
    rpc_call("list", Profile::RpcReadHeavy, json!({}))
}

pub fn build_get_page_rpc_call(slug: &str) -> Result<RpcCall, ContentError> {
    let slug = slug.trim();
    if slug.is_empty() {
        return Err(ContentError::SlugRequired);
    }
    Ok(rpc_call(
        "get",
        Profile::RpcReadHeavy,
        json!({ "slug": slug }),
    ))
}

pub fn build_update_page_rpc_call(params: UpdatePageParams) -> Result<RpcCall, ContentError> {
    let slug = params.slug.trim();
    if slug.is_empty() {
        return Err(ContentError::SlugRequired);
    }
    let mut payload = Map::new();
    payload.insert("slug".to_string(), Value::String(slug.to_string()));
    if !params.title.trim().is_empty() {
        payload.insert(
            "title".to_string(),
            Value::String(params.title.trim().to_string()),
        );
    }
    if let Some(body) = params.body {
        payload.insert("body".to_string(), Value::String(body));
    }
    if let Some(visibility) = params.visibility {
        payload.insert(
            "visibility".to_string(),
            Value::String(normalize_visibility(&visibility, true)?),
        );
    }
    if payload.len() == 1 {
        return Err(ContentError::NoUpdateFields);
    }
    Ok(rpc_call(
        "update",
        Profile::RpcDefault,
        Value::Object(payload),
    ))
}

pub fn build_rename_page_rpc_call(params: RenamePageParams) -> Result<RpcCall, ContentError> {
    let slug = params.slug.trim();
    let target = params.to.trim();
    if slug.is_empty() || target.is_empty() {
        return Err(ContentError::SlugRequired);
    }
    Ok(rpc_call(
        "rename",
        Profile::RpcDefault,
        json!({ "old_slug": slug, "new_slug": target }),
    ))
}

pub fn build_delete_page_rpc_call(slug: &str) -> Result<RpcCall, ContentError> {
    let slug = slug.trim();
    if slug.is_empty() {
        return Err(ContentError::SlugRequired);
    }
    Ok(rpc_call(
        "delete",
        Profile::RpcDefault,
        json!({ "slug": slug }),
    ))
}

pub fn normalize_visibility(value: &str, empty_allowed: bool) -> Result<String, ContentError> {
    let visibility = value.trim().to_ascii_lowercase();
    if visibility.is_empty() && empty_allowed {
        return Ok(String::new());
    }
    if visibility.is_empty() {
        return Ok("public".to_string());
    }
    match visibility.as_str() {
        "public" | "draft" | "unlisted" => Ok(visibility),
        _ => Err(ContentError::VisibilityInvalid),
    }
}

pub fn create_page_summary(slug: &str) -> String {
    format!("Created content page {}", slug.trim())
}

pub fn list_pages_summary(result: &Value) -> String {
    format!("Fetched {} content pages", result_count(result))
}

pub fn get_page_summary(slug: &str) -> String {
    format!("Fetched content page {}", slug.trim())
}

pub fn update_page_summary(slug: &str) -> String {
    format!("Updated content page {}", slug.trim())
}

pub fn rename_page_summary(slug: &str, target: &str) -> String {
    format!("Renamed content page {} to {}", slug.trim(), target.trim())
}

pub fn delete_page_summary(slug: &str) -> String {
    format!("Deleted content page {}", slug.trim())
}

pub fn page_action_result(action: &str, identity: &IdentitySummary, page: Value) -> CommandResult {
    CommandResult {
        data: json!({
            "action": action,
            "identity": identity_value(identity),
            "page": page,
        }),
        summary: String::new(),
        warnings: Vec::new(),
    }
}

pub fn read_heavy_page_action_result(
    action: &str,
    identity: &IdentitySummary,
    page: Value,
) -> CommandResult {
    page_action_result(action, identity, page)
}

pub fn page_list_result(identity: &IdentitySummary, result: &Value) -> CommandResult {
    CommandResult {
        data: json!({
            "action": "list_pages",
            "identity": identity_value(identity),
            "pages": result.get("pages").cloned().unwrap_or(Value::Null),
            "count": result_count(result),
        }),
        summary: list_pages_summary(result),
        warnings: Vec::new(),
    }
}

pub fn page_update_result(
    identity: &IdentitySummary,
    changed_fields: Vec<String>,
    page: Value,
) -> CommandResult {
    CommandResult {
        data: json!({
            "action": "update_page",
            "identity": identity_value(identity),
            "changed_fields": changed_fields,
            "page": page,
        }),
        summary: String::new(),
        warnings: Vec::new(),
    }
}

pub fn page_rename_result(
    identity: &IdentitySummary,
    slug: &str,
    target: &str,
    page: Value,
) -> CommandResult {
    CommandResult {
        data: json!({
            "action": "rename_page",
            "identity": identity_value(identity),
            "from": slug.trim(),
            "to": target.trim(),
            "page": page,
        }),
        summary: String::new(),
        warnings: Vec::new(),
    }
}

pub fn page_delete_result(identity: &IdentitySummary, slug: &str, result: Value) -> CommandResult {
    CommandResult {
        data: json!({
            "action": "delete_page",
            "identity": identity_value(identity),
            "slug": slug.trim(),
            "result": result,
        }),
        summary: String::new(),
        warnings: Vec::new(),
    }
}

fn rpc_call(method: &'static str, profile: Profile, params: Value) -> RpcCall {
    RpcCall {
        endpoint: CONTENT_RPC_ENDPOINT,
        method,
        profile,
        params,
    }
}

fn result_count(result: &Value) -> i64 {
    match result.get("count") {
        Some(Value::Number(number)) => number
            .as_i64()
            .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok()))
            .or_else(|| number.as_f64().map(|value| value as i64))
            .unwrap_or(0),
        _ => 0,
    }
}

fn identity_value(identity: &IdentitySummary) -> Value {
    json!({
        "identity_name": identity.identity_name,
        "did": identity.did,
        "handle": identity.handle,
    })
}
