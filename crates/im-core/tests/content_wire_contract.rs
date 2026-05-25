use im_core::compat::content::{
    build_create_page_rpc_call, build_delete_page_rpc_call, build_get_page_rpc_call,
    build_list_pages_rpc_call, build_rename_page_rpc_call, build_update_page_rpc_call,
    TransportProfile, CONTENT_RPC_ENDPOINT,
};
use im_core::content::{ContentPageQuery, PageDraft, PageRef, PageSlug, PageUpdate, Visibility};
use serde_json::json;

#[test]
fn content_rpc_calls_match_go_methods_profiles_and_params() {
    assert_eq!(CONTENT_RPC_ENDPOINT, "/content/rpc");

    let create = build_create_page_rpc_call(
        PageDraft::new(
            PageSlug::parse(" hello ").expect("slug"),
            " Hello ",
            "# Body",
            Visibility::parse("DRAFT").expect("visibility"),
        )
        .expect("draft"),
    )
    .expect("create call");
    assert_eq!(create.endpoint, CONTENT_RPC_ENDPOINT);
    assert_eq!(create.method, "create");
    assert_eq!(create.profile, TransportProfile::RpcDefault);
    assert_eq!(
        create.params,
        json!({
            "slug": "hello",
            "title": "Hello",
            "body": "# Body",
            "visibility": "draft",
        })
    );

    let create_default_visibility = build_create_page_rpc_call(
        PageDraft::new(
            PageSlug::parse("hello").expect("slug"),
            "Hello",
            "",
            Visibility::parse("").expect("default visibility"),
        )
        .expect("draft"),
    )
    .expect("create default visibility");
    assert_eq!(create_default_visibility.params["visibility"], "public");

    let list = build_list_pages_rpc_call(ContentPageQuery::default());
    assert_eq!(list.method, "list");
    assert_eq!(list.profile, TransportProfile::RpcReadHeavy);
    assert_eq!(list.params, json!({}));

    let get = build_get_page_rpc_call(PageRef::new(PageSlug::parse(" hello ").expect("get slug")));
    assert_eq!(get.method, "get");
    assert_eq!(get.profile, TransportProfile::RpcReadHeavy);
    assert_eq!(get.params, json!({ "slug": "hello" }));

    let update = build_update_page_rpc_call(
        PageRef::new(PageSlug::parse(" hello ").expect("update slug")),
        PageUpdate {
            title: Some(" New ".to_string()),
            body: Some("Updated".to_string()),
            visibility: Some(Visibility::parse("UNLISTED").expect("visibility")),
        },
    )
    .expect("update call");
    assert_eq!(update.method, "update");
    assert_eq!(update.profile, TransportProfile::RpcDefault);
    assert_eq!(
        update.params,
        json!({
            "slug": "hello",
            "title": "New",
            "body": "Updated",
            "visibility": "unlisted",
        })
    );

    let rename = build_rename_page_rpc_call(
        PageRef::new(PageSlug::parse(" old ").expect("old slug")),
        PageSlug::parse(" new ").expect("new slug"),
    );
    assert_eq!(rename.method, "rename");
    assert_eq!(rename.profile, TransportProfile::RpcDefault);
    assert_eq!(
        rename.params,
        json!({ "old_slug": "old", "new_slug": "new" })
    );

    let delete =
        build_delete_page_rpc_call(PageRef::new(PageSlug::parse(" old ").expect("delete slug")));
    assert_eq!(delete.method, "delete");
    assert_eq!(delete.profile, TransportProfile::RpcDefault);
    assert_eq!(delete.params, json!({ "slug": "old" }));
}

#[test]
fn content_validation_matches_go_live_boundaries() {
    assert!(PageSlug::parse(" ").is_err());
    assert!(PageDraft::new(
        PageSlug::parse("hello").expect("slug"),
        " ",
        "",
        Visibility::Public,
    )
    .is_err());
    assert!(Visibility::parse("private").is_err());
    assert!(build_update_page_rpc_call(
        PageRef::new(PageSlug::parse("hello").expect("slug")),
        PageUpdate::default(),
    )
    .is_err());
    assert_eq!(Visibility::parse("").unwrap(), Visibility::Public);
    assert_eq!(
        Visibility::parse(" UNLISTED ").unwrap(),
        Visibility::Unlisted
    );
}

#[test]
fn content_normalizers_return_typed_documents_with_raw_payloads() {
    let page = im_core::compat::content::normalize_page(json!({
        "slug": "hello",
        "title": "Hello",
        "body": "# Body",
        "visibility": "draft",
        "extra": true,
    }))
    .expect("page");
    assert_eq!(page.slug.as_str(), "hello");
    assert_eq!(page.title.as_deref(), Some("Hello"));
    assert_eq!(page.visibility, Some(Visibility::Draft));
    assert_eq!(page.raw["extra"], true);

    let list = im_core::compat::content::normalize_page_list(json!({
        "count": 1,
        "pages": [{ "slug": "hello" }],
    }))
    .expect("list");
    assert_eq!(list.items.len(), 1);
    assert_eq!(list.items[0].slug.as_str(), "hello");
    assert!(!list.has_more);
    assert!(list.next_cursor.is_none());
}
