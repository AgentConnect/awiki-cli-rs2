use awiki_cli::content::{
    build_create_page_rpc_call, build_delete_page_rpc_call, build_get_page_rpc_call,
    build_list_pages_rpc_call, build_rename_page_rpc_call, build_update_page_rpc_call,
    create_page_summary, delete_page_summary, get_page_summary, list_pages_summary,
    normalize_visibility, page_action_result, page_delete_result, page_list_result,
    page_rename_result, page_update_result, rename_page_summary, update_page_summary, ContentError,
    CreatePageParams, IdentitySummary, RenamePageParams, UpdatePageParams, CONTENT_RPC_ENDPOINT,
    DID_AUTH_RPC_ENDPOINT,
};
use awiki_cli::transportcfg::Profile;
use serde_json::json;

#[test]
fn content_rpc_calls_match_go_methods_profiles_and_params() {
    assert_eq!(CONTENT_RPC_ENDPOINT, "/content/rpc");
    assert_eq!(DID_AUTH_RPC_ENDPOINT, "/user-service/did-auth/rpc");

    let create = build_create_page_rpc_call(CreatePageParams {
        slug: " hello ".to_string(),
        title: " Hello ".to_string(),
        body: "# Body".to_string(),
        visibility: "DRAFT".to_string(),
    })
    .expect("create call");
    assert_eq!(create.endpoint, CONTENT_RPC_ENDPOINT);
    assert_eq!(create.method, "create");
    assert_eq!(create.profile, Profile::RpcDefault);
    assert_eq!(
        create.params,
        json!({
            "slug": "hello",
            "title": "Hello",
            "body": "# Body",
            "visibility": "draft",
        })
    );

    let create_default_visibility = build_create_page_rpc_call(CreatePageParams {
        slug: "hello".to_string(),
        title: "Hello".to_string(),
        body: String::new(),
        visibility: String::new(),
    })
    .expect("create default visibility");
    assert_eq!(create_default_visibility.params["visibility"], "public");

    let list = build_list_pages_rpc_call();
    assert_eq!(list.method, "list");
    assert_eq!(list.profile, Profile::RpcReadHeavy);
    assert_eq!(list.params, json!({}));

    let get = build_get_page_rpc_call(" hello ").expect("get call");
    assert_eq!(get.method, "get");
    assert_eq!(get.profile, Profile::RpcReadHeavy);
    assert_eq!(get.params, json!({ "slug": "hello" }));

    let update = build_update_page_rpc_call(UpdatePageParams {
        slug: " hello ".to_string(),
        title: " New ".to_string(),
        body: Some("Updated".to_string()),
        visibility: Some("UNLISTED".to_string()),
    })
    .expect("update call");
    assert_eq!(update.method, "update");
    assert_eq!(update.profile, Profile::RpcDefault);
    assert_eq!(
        update.params,
        json!({
            "slug": "hello",
            "title": "New",
            "body": "Updated",
            "visibility": "unlisted",
        })
    );

    let update_empty_visibility = build_update_page_rpc_call(UpdatePageParams {
        slug: "hello".to_string(),
        visibility: Some(" ".to_string()),
        ..Default::default()
    })
    .expect("update empty visibility");
    assert_eq!(
        update_empty_visibility.params,
        json!({ "slug": "hello", "visibility": "" })
    );

    let rename = build_rename_page_rpc_call(RenamePageParams {
        slug: " old ".to_string(),
        to: " new ".to_string(),
    })
    .expect("rename call");
    assert_eq!(rename.method, "rename");
    assert_eq!(rename.profile, Profile::RpcDefault);
    assert_eq!(
        rename.params,
        json!({ "old_slug": "old", "new_slug": "new" })
    );

    let delete = build_delete_page_rpc_call(" old ").expect("delete call");
    assert_eq!(delete.method, "delete");
    assert_eq!(delete.profile, Profile::RpcDefault);
    assert_eq!(delete.params, json!({ "slug": "old" }));
}

#[test]
fn content_service_validation_matches_go_live_boundaries() {
    assert!(matches!(
        build_create_page_rpc_call(CreatePageParams::default()).expect_err("missing slug"),
        ContentError::SlugRequired
    ));
    assert!(matches!(
        build_create_page_rpc_call(CreatePageParams {
            slug: "hello".to_string(),
            ..Default::default()
        })
        .expect_err("missing title"),
        ContentError::TitleRequired
    ));
    assert!(matches!(
        build_create_page_rpc_call(CreatePageParams {
            slug: "hello".to_string(),
            title: "Hello".to_string(),
            visibility: "private".to_string(),
            ..Default::default()
        })
        .expect_err("invalid visibility"),
        ContentError::VisibilityInvalid
    ));
    assert!(matches!(
        build_update_page_rpc_call(UpdatePageParams {
            slug: "hello".to_string(),
            ..Default::default()
        })
        .expect_err("no update fields"),
        ContentError::NoUpdateFields
    ));
    assert!(matches!(
        build_update_page_rpc_call(UpdatePageParams {
            slug: "hello".to_string(),
            visibility: Some("private".to_string()),
            ..Default::default()
        })
        .expect_err("invalid update visibility"),
        ContentError::VisibilityInvalid
    ));
    assert!(matches!(
        build_rename_page_rpc_call(RenamePageParams {
            slug: "old".to_string(),
            to: " ".to_string(),
        })
        .expect_err("missing target"),
        ContentError::SlugRequired
    ));
    assert!(matches!(
        build_delete_page_rpc_call(" ").expect_err("missing delete slug"),
        ContentError::SlugRequired
    ));

    assert_eq!(normalize_visibility("", false).unwrap(), "public");
    assert_eq!(normalize_visibility("", true).unwrap(), "");
    assert_eq!(
        normalize_visibility(" UNLISTED ", false).unwrap(),
        "unlisted"
    );
    assert!(matches!(
        normalize_visibility("private", false).expect_err("invalid visibility"),
        ContentError::VisibilityInvalid
    ));
}

#[test]
fn content_summaries_and_result_shapes_match_go_service_contract() {
    assert_eq!(create_page_summary(" hello "), "Created content page hello");
    assert_eq!(
        list_pages_summary(&json!({ "count": 2.0, "pages": [] })),
        "Fetched 2 content pages"
    );
    assert_eq!(get_page_summary(" hello "), "Fetched content page hello");
    assert_eq!(update_page_summary(" hello "), "Updated content page hello");
    assert_eq!(
        rename_page_summary(" old ", " new "),
        "Renamed content page old to new"
    );
    assert_eq!(delete_page_summary(" old "), "Deleted content page old");

    let identity = IdentitySummary {
        identity_name: "alice".to_string(),
        did: "did:wba:example.com:user:alice:e1".to_string(),
        handle: "alice".to_string(),
    };
    let list = page_list_result(
        &identity,
        &json!({ "count": 1, "pages": [{ "slug": "hello" }] }),
    );
    assert_eq!(list.summary, "Fetched 1 content pages");
    assert_eq!(list.warnings.len(), 0);
    assert_eq!(
        list.data,
        json!({
            "action": "list_pages",
            "identity": {
                "identity_name": "alice",
                "did": "did:wba:example.com:user:alice:e1",
                "handle": "alice",
            },
            "pages": [{ "slug": "hello" }],
            "count": 1,
        })
    );

    let action = page_action_result("get_page", &identity, json!({ "slug": "hello" }));
    assert_eq!(
        action.data,
        json!({
            "action": "get_page",
            "identity": {
                "identity_name": "alice",
                "did": "did:wba:example.com:user:alice:e1",
                "handle": "alice",
            },
            "page": { "slug": "hello" },
        })
    );

    let update = page_update_result(
        &identity,
        vec![
            "title".to_string(),
            "body".to_string(),
            "visibility".to_string(),
        ],
        json!({ "slug": "hello", "title": "New" }),
    );
    assert_eq!(
        update.data,
        json!({
            "action": "update_page",
            "identity": {
                "identity_name": "alice",
                "did": "did:wba:example.com:user:alice:e1",
                "handle": "alice",
            },
            "changed_fields": ["title", "body", "visibility"],
            "page": { "slug": "hello", "title": "New" },
        })
    );

    let rename = page_rename_result(&identity, " old ", " new ", json!({ "slug": "new" }));
    assert_eq!(
        rename.data,
        json!({
            "action": "rename_page",
            "identity": {
                "identity_name": "alice",
                "did": "did:wba:example.com:user:alice:e1",
                "handle": "alice",
            },
            "from": "old",
            "to": "new",
            "page": { "slug": "new" },
        })
    );

    let delete = page_delete_result(&identity, " old ", json!({ "deleted": true }));
    assert_eq!(
        delete.data,
        json!({
            "action": "delete_page",
            "identity": {
                "identity_name": "alice",
                "did": "did:wba:example.com:user:alice:e1",
                "handle": "alice",
            },
            "slug": "old",
            "result": { "deleted": true },
        })
    );
}
