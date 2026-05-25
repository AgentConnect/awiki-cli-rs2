use im_core::content::PageSlug;
use im_core::site::{
    SiteDomain, SitePageDraft, SitePageQuery, SitePageRef, SitePageUpdate, SiteRootDraft,
};
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct CommandResult {
    pub data: Value,
    pub summary: String,
    pub warnings: Vec<String>,
}

pub fn get_root(client: &im_core::ImClient, domain: String) -> im_core::ImResult<CommandResult> {
    let domain = SiteDomain::parse(domain)?;
    let root = client.site().get_root(domain.clone())?;
    Ok(CommandResult {
        data: json!({
            "action": "site_root_get",
            "identity": identity_value(client.current_identity()),
            "root": root.raw,
        }),
        summary: get_root_summary(domain.as_str()),
        warnings: Vec::new(),
    })
}

pub fn set_root(
    client: &im_core::ImClient,
    domain: String,
    body: String,
) -> im_core::ImResult<CommandResult> {
    let domain = SiteDomain::parse(domain)?;
    let root = client.site().set_root(SiteRootDraft {
        domain: domain.clone(),
        body,
    })?;
    Ok(CommandResult {
        data: json!({
            "action": "site_root_set",
            "identity": identity_value(client.current_identity()),
            "root": root.raw,
        }),
        summary: set_root_summary(domain.as_str()),
        warnings: Vec::new(),
    })
}

pub fn list_pages(client: &im_core::ImClient, domain: String) -> im_core::ImResult<CommandResult> {
    let domain = SiteDomain::parse(domain)?;
    let page = client.site().list_pages(SitePageQuery {
        domain: domain.clone(),
        ..Default::default()
    })?;
    let pages = page
        .items
        .into_iter()
        .map(|item| item.raw)
        .collect::<Vec<_>>();
    let count = pages.len();
    Ok(CommandResult {
        data: json!({
            "action": "site_page_list",
            "identity": identity_value(client.current_identity()),
            "domain": domain.as_str(),
            "pages": pages,
            "count": count,
        }),
        summary: format!("Fetched {count} site pages for {}", domain.as_str()),
        warnings: Vec::new(),
    })
}

pub fn get_page(
    client: &im_core::ImClient,
    domain: String,
    slug: String,
) -> im_core::ImResult<CommandResult> {
    let page_ref = page_ref(domain, slug)?;
    let page = client.site().get_page(page_ref.clone())?;
    Ok(page_action_result(
        "site_page_get",
        client.current_identity(),
        page.raw,
        get_page_summary(page_ref.domain.as_str(), page_ref.slug.as_str()),
    ))
}

pub fn create_page(
    client: &im_core::ImClient,
    domain: String,
    slug: String,
    body: String,
) -> im_core::ImResult<CommandResult> {
    let domain = SiteDomain::parse(domain)?;
    let slug = PageSlug::parse(slug)?;
    let page = client.site().create_page(SitePageDraft {
        domain: domain.clone(),
        slug: slug.clone(),
        body,
    })?;
    Ok(page_action_result(
        "site_page_create",
        client.current_identity(),
        page.raw,
        create_page_summary(domain.as_str(), slug.as_str()),
    ))
}

pub fn update_page(
    client: &im_core::ImClient,
    domain: String,
    slug: String,
    body: String,
) -> im_core::ImResult<CommandResult> {
    let page_ref = page_ref(domain, slug)?;
    let page = client
        .site()
        .update_page(page_ref.clone(), SitePageUpdate { body })?;
    Ok(page_action_result(
        "site_page_update",
        client.current_identity(),
        page.raw,
        update_page_summary(page_ref.domain.as_str(), page_ref.slug.as_str()),
    ))
}

pub fn rename_page(
    client: &im_core::ImClient,
    domain: String,
    slug: String,
    target: String,
) -> im_core::ImResult<CommandResult> {
    let page_ref = page_ref(domain, slug)?;
    let target = PageSlug::parse(target)?;
    let page = client
        .site()
        .rename_page(page_ref.clone(), target.clone())?;
    Ok(CommandResult {
        data: json!({
            "action": "site_page_rename",
            "identity": identity_value(client.current_identity()),
            "from": page_ref.slug.as_str(),
            "to": target.as_str(),
            "page": page.raw,
        }),
        summary: rename_page_summary(
            page_ref.domain.as_str(),
            page_ref.slug.as_str(),
            target.as_str(),
        ),
        warnings: Vec::new(),
    })
}

pub fn delete_page(
    client: &im_core::ImClient,
    domain: String,
    slug: String,
) -> im_core::ImResult<CommandResult> {
    let page_ref = page_ref(domain, slug)?;
    let result = client.site().delete_page(page_ref.clone())?;
    Ok(CommandResult {
        data: json!({
            "action": "site_page_delete",
            "identity": identity_value(client.current_identity()),
            "domain": page_ref.domain.as_str(),
            "slug": page_ref.slug.as_str(),
            "result": result.raw,
        }),
        summary: delete_page_summary(page_ref.domain.as_str(), page_ref.slug.as_str()),
        warnings: Vec::new(),
    })
}

fn page_ref(domain: String, slug: String) -> im_core::ImResult<SitePageRef> {
    Ok(SitePageRef::new(
        SiteDomain::parse(domain)?,
        PageSlug::parse(slug)?,
    ))
}

fn page_action_result(
    action: &str,
    identity: &im_core::identity::IdentitySummary,
    page: Value,
    summary: String,
) -> CommandResult {
    CommandResult {
        data: json!({
            "action": action,
            "identity": identity_value(identity),
            "page": page,
        }),
        summary,
        warnings: Vec::new(),
    }
}

fn identity_value(identity: &im_core::identity::IdentitySummary) -> Value {
    json!({
        "identity_name": identity.local_alias.as_deref().unwrap_or(identity.id.as_str()),
        "did": identity.did.as_str(),
        "handle": identity.handle.as_ref().map(|handle| handle.as_str()).unwrap_or_default(),
    })
}

fn get_root_summary(domain: &str) -> String {
    format!("Fetched site root for {}", domain.trim())
}

fn set_root_summary(domain: &str) -> String {
    format!("Updated site root for {}", domain.trim())
}

fn get_page_summary(domain: &str, slug: &str) -> String {
    format!("Fetched site page {} for {}", slug.trim(), domain.trim())
}

fn create_page_summary(domain: &str, slug: &str) -> String {
    format!("Created site page {} for {}", slug.trim(), domain.trim())
}

fn update_page_summary(domain: &str, slug: &str) -> String {
    format!("Updated site page {} for {}", slug.trim(), domain.trim())
}

fn rename_page_summary(domain: &str, slug: &str, target: &str) -> String {
    format!(
        "Renamed site page {} to {} for {}",
        slug.trim(),
        target.trim(),
        domain.trim()
    )
}

fn delete_page_summary(domain: &str, slug: &str) -> String {
    format!("Deleted site page {} for {}", slug.trim(), domain.trim())
}
