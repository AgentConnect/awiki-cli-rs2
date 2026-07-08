use im_core::content::{ContentPageQuery, PageDraft, PageRef, PageSlug, PageUpdate, Visibility};
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct CommandResult {
    pub data: Value,
    pub summary: String,
    pub warnings: Vec<String>,
}

pub fn create_page(
    client: &im_core::ImClient,
    slug: String,
    title: String,
    body: String,
    visibility: String,
) -> im_core::ImResult<CommandResult> {
    let slug = PageSlug::parse(slug)?;
    let visibility = Visibility::parse(visibility)?;
    let page =
        client
            .content()
            .create_page(PageDraft::new(slug.clone(), title, body, visibility)?)?;
    Ok(page_action_result(
        "create_page",
        client.current_identity(),
        page.raw,
        create_page_summary(slug.as_str()),
    ))
}

pub async fn create_page_async(
    client: &im_core::ImClient,
    slug: String,
    title: String,
    body: String,
    visibility: String,
) -> im_core::ImResult<CommandResult> {
    let slug = PageSlug::parse(slug)?;
    let visibility = Visibility::parse(visibility)?;
    let page = client
        .content()
        .create_page_async(PageDraft::new(slug.clone(), title, body, visibility)?)
        .await?;
    Ok(page_action_result(
        "create_page",
        client.current_identity(),
        page.raw,
        create_page_summary(slug.as_str()),
    ))
}

pub fn list_pages(client: &im_core::ImClient) -> im_core::ImResult<CommandResult> {
    let page = client.content().list_pages(ContentPageQuery::default())?;
    let pages = page
        .items
        .into_iter()
        .map(|item| item.raw)
        .collect::<Vec<_>>();
    let count = pages.len();
    Ok(CommandResult {
        data: json!({
            "action": "list_pages",
            "identity": identity_value(client.current_identity()),
            "pages": pages,
            "count": count,
        }),
        summary: format!("Fetched {count} content pages"),
        warnings: Vec::new(),
    })
}

pub async fn list_pages_async(client: &im_core::ImClient) -> im_core::ImResult<CommandResult> {
    let page = client
        .content()
        .list_pages_async(ContentPageQuery::default())
        .await?;
    let pages = page
        .items
        .into_iter()
        .map(|item| item.raw)
        .collect::<Vec<_>>();
    let count = pages.len();
    Ok(CommandResult {
        data: json!({
            "action": "list_pages",
            "identity": identity_value(client.current_identity()),
            "pages": pages,
            "count": count,
        }),
        summary: format!("Fetched {count} content pages"),
        warnings: Vec::new(),
    })
}

pub fn get_page(client: &im_core::ImClient, slug: String) -> im_core::ImResult<CommandResult> {
    let slug = PageSlug::parse(slug)?;
    let page = client.content().get_page(PageRef::new(slug.clone()))?;
    Ok(page_action_result(
        "get_page",
        client.current_identity(),
        page.raw,
        get_page_summary(slug.as_str()),
    ))
}

pub async fn get_page_async(
    client: &im_core::ImClient,
    slug: String,
) -> im_core::ImResult<CommandResult> {
    let slug = PageSlug::parse(slug)?;
    let page = client
        .content()
        .get_page_async(PageRef::new(slug.clone()))
        .await?;
    Ok(page_action_result(
        "get_page",
        client.current_identity(),
        page.raw,
        get_page_summary(slug.as_str()),
    ))
}

pub fn update_page(
    client: &im_core::ImClient,
    slug: String,
    title: String,
    body: Option<String>,
    visibility: Option<String>,
) -> im_core::ImResult<CommandResult> {
    let slug = PageSlug::parse(slug)?;
    let patch = PageUpdate {
        title: Some(title).filter(|value| !value.trim().is_empty()),
        body,
        visibility: visibility
            .filter(|value| !value.trim().is_empty())
            .map(Visibility::parse)
            .transpose()?,
    };
    let changed_fields = update_changed_fields(&patch);
    let page = client
        .content()
        .update_page(PageRef::new(slug.clone()), patch)?;
    Ok(CommandResult {
        data: json!({
            "action": "update_page",
            "identity": identity_value(client.current_identity()),
            "changed_fields": changed_fields,
            "page": page.raw,
        }),
        summary: update_page_summary(slug.as_str()),
        warnings: Vec::new(),
    })
}

pub async fn update_page_async(
    client: &im_core::ImClient,
    slug: String,
    title: String,
    body: Option<String>,
    visibility: Option<String>,
) -> im_core::ImResult<CommandResult> {
    let slug = PageSlug::parse(slug)?;
    let patch = PageUpdate {
        title: Some(title).filter(|value| !value.trim().is_empty()),
        body,
        visibility: visibility
            .filter(|value| !value.trim().is_empty())
            .map(Visibility::parse)
            .transpose()?,
    };
    let changed_fields = update_changed_fields(&patch);
    let page = client
        .content()
        .update_page_async(PageRef::new(slug.clone()), patch)
        .await?;
    Ok(CommandResult {
        data: json!({
            "action": "update_page",
            "identity": identity_value(client.current_identity()),
            "changed_fields": changed_fields,
            "page": page.raw,
        }),
        summary: update_page_summary(slug.as_str()),
        warnings: Vec::new(),
    })
}

pub fn rename_page(
    client: &im_core::ImClient,
    slug: String,
    target: String,
) -> im_core::ImResult<CommandResult> {
    let slug = PageSlug::parse(slug)?;
    let target = PageSlug::parse(target)?;
    let page = client
        .content()
        .rename_page(PageRef::new(slug.clone()), target.clone())?;
    Ok(CommandResult {
        data: json!({
            "action": "rename_page",
            "identity": identity_value(client.current_identity()),
            "from": slug.as_str(),
            "to": target.as_str(),
            "page": page.raw,
        }),
        summary: rename_page_summary(slug.as_str(), target.as_str()),
        warnings: Vec::new(),
    })
}

pub async fn rename_page_async(
    client: &im_core::ImClient,
    slug: String,
    target: String,
) -> im_core::ImResult<CommandResult> {
    let slug = PageSlug::parse(slug)?;
    let target = PageSlug::parse(target)?;
    let page = client
        .content()
        .rename_page_async(PageRef::new(slug.clone()), target.clone())
        .await?;
    Ok(CommandResult {
        data: json!({
            "action": "rename_page",
            "identity": identity_value(client.current_identity()),
            "from": slug.as_str(),
            "to": target.as_str(),
            "page": page.raw,
        }),
        summary: rename_page_summary(slug.as_str(), target.as_str()),
        warnings: Vec::new(),
    })
}

pub fn delete_page(client: &im_core::ImClient, slug: String) -> im_core::ImResult<CommandResult> {
    let slug = PageSlug::parse(slug)?;
    let result = client.content().delete_page(PageRef::new(slug.clone()))?;
    Ok(CommandResult {
        data: json!({
            "action": "delete_page",
            "identity": identity_value(client.current_identity()),
            "slug": slug.as_str(),
            "result": result.raw,
        }),
        summary: delete_page_summary(slug.as_str()),
        warnings: Vec::new(),
    })
}

pub async fn delete_page_async(
    client: &im_core::ImClient,
    slug: String,
) -> im_core::ImResult<CommandResult> {
    let slug = PageSlug::parse(slug)?;
    let result = client
        .content()
        .delete_page_async(PageRef::new(slug.clone()))
        .await?;
    Ok(CommandResult {
        data: json!({
            "action": "delete_page",
            "identity": identity_value(client.current_identity()),
            "slug": slug.as_str(),
            "result": result.raw,
        }),
        summary: delete_page_summary(slug.as_str()),
        warnings: Vec::new(),
    })
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

fn update_changed_fields(patch: &PageUpdate) -> Vec<String> {
    let mut changed_fields = Vec::new();
    if patch.title.is_some() {
        changed_fields.push("title".to_string());
    }
    if patch.body.is_some() {
        changed_fields.push("body".to_string());
    }
    if patch.visibility.is_some() {
        changed_fields.push("visibility".to_string());
    }
    changed_fields
}

fn identity_value(identity: &im_core::identity::IdentitySummary) -> Value {
    json!({
        "identity_name": identity.local_alias.as_deref().unwrap_or(identity.id.as_str()),
        "did": identity.did.as_str(),
        "handle": identity.handle.as_ref().map(|handle| handle.as_str()).unwrap_or_default(),
    })
}

fn create_page_summary(slug: &str) -> String {
    format!("Created content page {}", slug.trim())
}

fn get_page_summary(slug: &str) -> String {
    format!("Fetched content page {}", slug.trim())
}

fn update_page_summary(slug: &str) -> String {
    format!("Updated content page {}", slug.trim())
}

fn rename_page_summary(slug: &str, target: &str) -> String {
    format!("Renamed content page {} to {}", slug.trim(), target.trim())
}

fn delete_page_summary(slug: &str) -> String {
    format!("Deleted content page {}", slug.trim())
}
