use super::types::{
    CommandResult, CreatePageParams, IdentitySummary, RenamePageParams, SetRootParams, SiteError,
    UpdatePageParams, SITE_RPC_ENDPOINT,
};
use crate::config;
use crate::transportcfg::Profile;
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct RpcCall {
    pub endpoint: &'static str,
    pub method: &'static str,
    pub profile: Profile,
    pub params: Value,
}

pub fn build_get_root_rpc_call(domain: &str) -> Result<RpcCall, SiteError> {
    let domain = normalize_live_domain(domain)?;
    Ok(rpc_call(
        "get_root",
        Profile::RpcReadHeavy,
        json!({ "domain": domain }),
    ))
}

pub fn build_set_root_rpc_call(params: SetRootParams) -> Result<RpcCall, SiteError> {
    let domain = normalize_live_domain(&params.domain)?;
    Ok(rpc_call(
        "set_root",
        Profile::RpcDefault,
        json!({ "domain": domain, "body": params.body }),
    ))
}

pub fn build_list_pages_rpc_call(domain: &str) -> Result<RpcCall, SiteError> {
    let domain = normalize_live_domain(domain)?;
    Ok(rpc_call(
        "list_pages",
        Profile::RpcReadHeavy,
        json!({ "domain": domain }),
    ))
}

pub fn build_get_page_rpc_call(domain: &str, slug: &str) -> Result<RpcCall, SiteError> {
    let domain = normalize_live_domain(domain)?;
    let slug = normalize_slug(slug)?;
    Ok(rpc_call(
        "get_page",
        Profile::RpcReadHeavy,
        json!({ "domain": domain, "slug": slug }),
    ))
}

pub fn build_create_page_rpc_call(params: CreatePageParams) -> Result<RpcCall, SiteError> {
    let domain = normalize_live_domain(&params.domain)?;
    let slug = normalize_slug(&params.slug)?;
    Ok(rpc_call(
        "create_page",
        Profile::RpcDefault,
        json!({ "domain": domain, "slug": slug, "body": params.body }),
    ))
}

pub fn build_update_page_rpc_call(params: UpdatePageParams) -> Result<RpcCall, SiteError> {
    let domain = normalize_live_domain(&params.domain)?;
    let slug = normalize_slug(&params.slug)?;
    Ok(rpc_call(
        "update_page",
        Profile::RpcDefault,
        json!({ "domain": domain, "slug": slug, "body": params.body }),
    ))
}

pub fn build_rename_page_rpc_call(params: RenamePageParams) -> Result<RpcCall, SiteError> {
    let domain = normalize_live_domain(&params.domain)?;
    let slug = normalize_slug(&params.slug)?;
    let target = normalize_slug(&params.to)?;
    Ok(rpc_call(
        "rename_page",
        Profile::RpcDefault,
        json!({ "domain": domain, "old_slug": slug, "new_slug": target }),
    ))
}

pub fn build_delete_page_rpc_call(domain: &str, slug: &str) -> Result<RpcCall, SiteError> {
    let domain = normalize_live_domain(domain)?;
    let slug = normalize_slug(slug)?;
    Ok(rpc_call(
        "delete_page",
        Profile::RpcDefault,
        json!({ "domain": domain, "slug": slug }),
    ))
}

pub fn normalize_live_domain(value: &str) -> Result<String, SiteError> {
    let normalized = config::normalize_did_domain(value)
        .map_err(|err| SiteError::DomainInvalid(err.to_string()))?;
    if normalized.is_empty() {
        return Err(SiteError::DomainRequired);
    }
    Ok(normalized)
}

pub fn normalize_domain(value: &str) -> Result<String, SiteError> {
    normalize_live_domain(value)
}

pub fn normalize_slug(value: &str) -> Result<String, SiteError> {
    let slug = value.trim();
    if slug.is_empty() {
        return Err(SiteError::SlugRequired);
    }
    Ok(slug.to_string())
}

pub fn get_root_summary(domain: &str) -> String {
    format!("Fetched site root for {}", domain.trim())
}

pub fn set_root_summary(domain: &str) -> String {
    format!("Updated site root for {}", domain.trim())
}

pub fn list_pages_summary(result: &Value, domain: &str) -> String {
    format!(
        "Fetched {} site pages for {}",
        result_count(result),
        domain.trim()
    )
}

pub fn get_page_summary(domain: &str, slug: &str) -> String {
    format!("Fetched site page {} for {}", slug.trim(), domain.trim())
}

pub fn create_page_summary(domain: &str, slug: &str) -> String {
    format!("Created site page {} for {}", slug.trim(), domain.trim())
}

pub fn update_page_summary(domain: &str, slug: &str) -> String {
    format!("Updated site page {} for {}", slug.trim(), domain.trim())
}

pub fn rename_page_summary(domain: &str, slug: &str, target: &str) -> String {
    format!(
        "Renamed site page {} to {} for {}",
        slug.trim(),
        target.trim(),
        domain.trim()
    )
}

pub fn delete_page_summary(domain: &str, slug: &str) -> String {
    format!("Deleted site page {} for {}", slug.trim(), domain.trim())
}

pub fn site_root_action_result(
    action: &str,
    identity: &IdentitySummary,
    root: Value,
) -> CommandResult {
    CommandResult {
        data: json!({
            "action": action,
            "identity": identity_value(identity),
            "root": root,
        }),
        summary: String::new(),
        warnings: Vec::new(),
    }
}

pub fn root_get_result(identity: &IdentitySummary, domain: &str, root: Value) -> CommandResult {
    let mut result = site_root_action_result("site_root_get", identity, root);
    result.summary = get_root_summary(domain);
    result
}

pub fn root_set_result(identity: &IdentitySummary, domain: &str, root: Value) -> CommandResult {
    let mut result = site_root_action_result("site_root_set", identity, root);
    result.summary = set_root_summary(domain);
    result
}

pub fn site_page_list_result(
    identity: &IdentitySummary,
    domain: &str,
    result: &Value,
) -> CommandResult {
    let normalized_domain = normalize_domain(domain).unwrap_or_else(|_| domain.trim().to_string());
    CommandResult {
        data: json!({
            "action": "site_page_list",
            "identity": identity_value(identity),
            "domain": normalized_domain,
            "pages": result.get("pages").cloned().unwrap_or(Value::Null),
            "count": result_count(result),
        }),
        summary: list_pages_summary(result, &normalized_domain),
        warnings: Vec::new(),
    }
}

pub fn page_list_result(identity: &IdentitySummary, domain: &str, result: &Value) -> CommandResult {
    site_page_list_result(identity, domain, result)
}

pub fn site_page_action_result(
    action: &str,
    identity: &IdentitySummary,
    page: Value,
) -> CommandResult {
    CommandResult {
        data: page_data(action, identity, page),
        summary: String::new(),
        warnings: Vec::new(),
    }
}

pub fn page_get_result(
    identity: &IdentitySummary,
    domain: &str,
    slug: &str,
    page: Value,
) -> CommandResult {
    CommandResult {
        data: site_page_action_result("site_page_get", identity, page).data,
        summary: get_page_summary(domain, slug),
        warnings: Vec::new(),
    }
}

pub fn page_create_result(
    identity: &IdentitySummary,
    domain: &str,
    slug: &str,
    page: Value,
) -> CommandResult {
    CommandResult {
        data: site_page_action_result("site_page_create", identity, page).data,
        summary: create_page_summary(domain, slug),
        warnings: Vec::new(),
    }
}

pub fn page_update_result(
    identity: &IdentitySummary,
    domain: &str,
    slug: &str,
    page: Value,
) -> CommandResult {
    CommandResult {
        data: site_page_action_result("site_page_update", identity, page).data,
        summary: update_page_summary(domain, slug),
        warnings: Vec::new(),
    }
}

pub fn site_page_rename_result(
    identity: &IdentitySummary,
    slug: &str,
    target: &str,
    page: Value,
) -> CommandResult {
    CommandResult {
        data: json!({
            "action": "site_page_rename",
            "identity": identity_value(identity),
            "from": slug.trim(),
            "to": target.trim(),
            "page": page,
        }),
        summary: String::new(),
        warnings: Vec::new(),
    }
}

pub fn page_rename_result(
    identity: &IdentitySummary,
    domain: &str,
    slug: &str,
    target: &str,
    page: Value,
) -> CommandResult {
    let mut result = site_page_rename_result(identity, slug, target, page);
    result.summary = rename_page_summary(domain, slug, target);
    result
}

pub fn site_page_delete_result(
    identity: &IdentitySummary,
    domain: &str,
    slug: &str,
    result: Value,
) -> CommandResult {
    let normalized_domain = normalize_domain(domain).unwrap_or_else(|_| domain.trim().to_string());
    CommandResult {
        data: json!({
            "action": "site_page_delete",
            "identity": identity_value(identity),
            "domain": normalized_domain,
            "slug": slug.trim(),
            "result": result,
        }),
        summary: String::new(),
        warnings: Vec::new(),
    }
}

pub fn page_delete_result(
    identity: &IdentitySummary,
    domain: &str,
    slug: &str,
    result: Value,
) -> CommandResult {
    let normalized_domain = normalize_domain(domain).unwrap_or_else(|_| domain.trim().to_string());
    let mut command_result = site_page_delete_result(identity, &normalized_domain, slug, result);
    command_result.summary = delete_page_summary(&normalized_domain, slug);
    command_result
}

fn rpc_call(method: &'static str, profile: Profile, params: Value) -> RpcCall {
    RpcCall {
        endpoint: SITE_RPC_ENDPOINT,
        method,
        profile,
        params,
    }
}

fn page_data(action: &str, identity: &IdentitySummary, page: Value) -> Value {
    json!({
        "action": action,
        "identity": identity_value(identity),
        "page": page,
    })
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
