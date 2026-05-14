use awiki_cli::site::{
    build_create_page_rpc_call, build_delete_page_rpc_call, build_get_page_rpc_call,
    build_get_root_rpc_call, build_list_pages_rpc_call, build_rename_page_rpc_call,
    build_set_root_rpc_call, build_update_page_rpc_call, create_page_summary, delete_page_summary,
    get_page_summary, get_root_summary, list_pages_summary, normalize_domain, rename_page_summary,
    set_root_summary, site_page_action_result, site_page_delete_result, site_page_list_result,
    site_page_rename_result, site_root_action_result, update_page_summary, CreatePageParams,
    IdentitySummary, RenamePageParams, SetRootParams, SiteError, UpdatePageParams,
    DID_AUTH_RPC_ENDPOINT, SITE_RPC_ENDPOINT,
};
use awiki_cli::transportcfg::Profile;
use serde_json::json;

#[test]
fn site_rpc_calls_match_go_methods_profiles_and_params() {
    assert_eq!(SITE_RPC_ENDPOINT, "/site/rpc");
    assert_eq!(DID_AUTH_RPC_ENDPOINT, "/user-service/did-auth/rpc");

    let get_root = build_get_root_rpc_call(" Tenant.Example. ").expect("get root call");
    assert_eq!(get_root.endpoint, SITE_RPC_ENDPOINT);
    assert_eq!(get_root.method, "get_root");
    assert_eq!(get_root.profile, Profile::RpcReadHeavy);
    assert_eq!(get_root.params, json!({ "domain": "tenant.example" }));

    let set_root = build_set_root_rpc_call(SetRootParams {
        domain: "Tenant.Example.".to_string(),
        body: String::new(),
    })
    .expect("set root call with explicit empty body");
    assert_eq!(set_root.endpoint, SITE_RPC_ENDPOINT);
    assert_eq!(set_root.method, "set_root");
    assert_eq!(set_root.profile, Profile::RpcDefault);
    assert_eq!(
        set_root.params,
        json!({ "domain": "tenant.example", "body": "" })
    );

    let list_pages = build_list_pages_rpc_call("Tenant.Example.").expect("list pages call");
    assert_eq!(list_pages.endpoint, SITE_RPC_ENDPOINT);
    assert_eq!(list_pages.method, "list_pages");
    assert_eq!(list_pages.profile, Profile::RpcReadHeavy);
    assert_eq!(list_pages.params, json!({ "domain": "tenant.example" }));

    let get_page = build_get_page_rpc_call("Tenant.Example.", " hello ").expect("get page call");
    assert_eq!(get_page.endpoint, SITE_RPC_ENDPOINT);
    assert_eq!(get_page.method, "get_page");
    assert_eq!(get_page.profile, Profile::RpcReadHeavy);
    assert_eq!(
        get_page.params,
        json!({ "domain": "tenant.example", "slug": "hello" })
    );

    let create_page = build_create_page_rpc_call(CreatePageParams {
        domain: "Tenant.Example.".to_string(),
        slug: " hello ".to_string(),
        body: String::new(),
    })
    .expect("create page call with explicit empty body");
    assert_eq!(create_page.endpoint, SITE_RPC_ENDPOINT);
    assert_eq!(create_page.method, "create_page");
    assert_eq!(create_page.profile, Profile::RpcDefault);
    assert_eq!(
        create_page.params,
        json!({ "domain": "tenant.example", "slug": "hello", "body": "" })
    );

    let update_page = build_update_page_rpc_call(UpdatePageParams {
        domain: "Tenant.Example.".to_string(),
        slug: " hello ".to_string(),
        body: String::new(),
    })
    .expect("update page call with explicit empty body");
    assert_eq!(update_page.endpoint, SITE_RPC_ENDPOINT);
    assert_eq!(update_page.method, "update_page");
    assert_eq!(update_page.profile, Profile::RpcDefault);
    assert_eq!(
        update_page.params,
        json!({ "domain": "tenant.example", "slug": "hello", "body": "" })
    );

    let rename_page = build_rename_page_rpc_call(RenamePageParams {
        domain: "Tenant.Example.".to_string(),
        slug: " old ".to_string(),
        to: " new ".to_string(),
    })
    .expect("rename page call");
    assert_eq!(rename_page.endpoint, SITE_RPC_ENDPOINT);
    assert_eq!(rename_page.method, "rename_page");
    assert_eq!(rename_page.profile, Profile::RpcDefault);
    assert_eq!(
        rename_page.params,
        json!({
            "domain": "tenant.example",
            "old_slug": "old",
            "new_slug": "new",
        })
    );

    let delete_page =
        build_delete_page_rpc_call("Tenant.Example.", " old ").expect("delete page call");
    assert_eq!(delete_page.endpoint, SITE_RPC_ENDPOINT);
    assert_eq!(delete_page.method, "delete_page");
    assert_eq!(delete_page.profile, Profile::RpcDefault);
    assert_eq!(
        delete_page.params,
        json!({ "domain": "tenant.example", "slug": "old" })
    );
}

#[test]
fn site_wire_validation_matches_go_live_boundaries() {
    assert_eq!(
        normalize_domain(" Tenant.Example. ").expect("normalized live domain"),
        "tenant.example"
    );

    assert_error_contains(
        normalize_domain(" ").expect_err("missing domain"),
        "did_domain is required",
    );
    assert_error_contains(
        normalize_domain("https://tenant.example").expect_err("URL-like domain"),
        "bare domain",
    );
    assert_error_contains(
        build_get_root_rpc_call("https://tenant.example").expect_err("URL-like RPC domain"),
        "bare domain",
    );

    assert_error_contains(
        build_get_root_rpc_call(" ").expect_err("missing root domain"),
        "did_domain is required",
    );
    assert_error_contains(
        build_set_root_rpc_call(SetRootParams {
            domain: " ".to_string(),
            body: String::new(),
        })
        .expect_err("missing set-root domain"),
        "did_domain is required",
    );
    assert_error_contains(
        build_list_pages_rpc_call(" ").expect_err("missing list domain"),
        "did_domain is required",
    );
    assert_error_contains(
        build_get_page_rpc_call(" ", "hello").expect_err("missing get-page domain"),
        "did_domain is required",
    );
    assert_error_contains(
        build_create_page_rpc_call(CreatePageParams {
            domain: " ".to_string(),
            slug: "hello".to_string(),
            body: String::new(),
        })
        .expect_err("missing create-page domain"),
        "did_domain is required",
    );
    assert_error_contains(
        build_delete_page_rpc_call(" ", "hello").expect_err("missing delete-page domain"),
        "did_domain is required",
    );

    assert!(matches!(
        build_get_page_rpc_call("tenant.example", " ").expect_err("missing get-page slug"),
        SiteError::SlugRequired
    ));
    assert!(matches!(
        build_create_page_rpc_call(CreatePageParams {
            domain: "tenant.example".to_string(),
            slug: " ".to_string(),
            body: String::new(),
        })
        .expect_err("missing create-page slug"),
        SiteError::SlugRequired
    ));
    assert!(matches!(
        build_update_page_rpc_call(UpdatePageParams {
            domain: "tenant.example".to_string(),
            slug: " ".to_string(),
            body: String::new(),
        })
        .expect_err("missing update-page slug"),
        SiteError::SlugRequired
    ));
    assert!(matches!(
        build_rename_page_rpc_call(RenamePageParams {
            domain: "tenant.example".to_string(),
            slug: " ".to_string(),
            to: "target".to_string(),
        })
        .expect_err("missing source rename slug"),
        SiteError::SlugRequired
    ));
    assert!(matches!(
        build_rename_page_rpc_call(RenamePageParams {
            domain: "tenant.example".to_string(),
            slug: "source".to_string(),
            to: " ".to_string(),
        })
        .expect_err("missing target rename slug"),
        SiteError::SlugRequired
    ));
    assert!(matches!(
        build_delete_page_rpc_call("tenant.example", " ").expect_err("missing delete-page slug"),
        SiteError::SlugRequired
    ));
}

#[test]
fn site_summaries_match_go_service_contracts() {
    assert_eq!(
        get_root_summary("tenant.example"),
        "Fetched site root for tenant.example"
    );
    assert_eq!(
        set_root_summary("tenant.example"),
        "Updated site root for tenant.example"
    );
    assert_eq!(
        list_pages_summary(&json!({ "count": 2.0, "pages": [] }), "tenant.example"),
        "Fetched 2 site pages for tenant.example"
    );
    assert_eq!(
        list_pages_summary(&json!({ "pages": [] }), "tenant.example"),
        "Fetched 0 site pages for tenant.example"
    );
    assert_eq!(
        get_page_summary("tenant.example", " hello "),
        "Fetched site page hello for tenant.example"
    );
    assert_eq!(
        create_page_summary("tenant.example", " hello "),
        "Created site page hello for tenant.example"
    );
    assert_eq!(
        update_page_summary("tenant.example", " hello "),
        "Updated site page hello for tenant.example"
    );
    assert_eq!(
        rename_page_summary("tenant.example", " old ", " new "),
        "Renamed site page old to new for tenant.example"
    );
    assert_eq!(
        delete_page_summary("tenant.example", " old "),
        "Deleted site page old for tenant.example"
    );
}

#[test]
fn site_result_shapes_match_go_service_contracts() {
    let identity = IdentitySummary {
        identity_name: "alice".to_string(),
        did: "did:wba:example.com:user:alice:e1".to_string(),
        handle: "alice".to_string(),
    };

    let root_get = site_root_action_result(
        "site_root_get",
        &identity,
        json!({ "domain": "tenant.example", "kind": "root" }),
    );
    assert_eq!(root_get.summary, "");
    assert_eq!(root_get.warnings.len(), 0);
    assert_eq!(
        root_get.data,
        json!({
            "action": "site_root_get",
            "identity": identity_json(),
            "root": { "domain": "tenant.example", "kind": "root" },
        })
    );

    let root_set = site_root_action_result(
        "site_root_set",
        &identity,
        json!({ "domain": "tenant.example", "body": "# Home" }),
    );
    assert_eq!(root_set.data["action"], "site_root_set");
    assert_eq!(
        root_set.data["root"],
        json!({ "domain": "tenant.example", "body": "# Home" })
    );

    let list = site_page_list_result(
        &identity,
        " Tenant.Example. ",
        &json!({ "count": 1.0, "pages": [{ "slug": "hello" }] }),
    );
    assert_eq!(list.summary, "Fetched 1 site pages for tenant.example");
    assert_eq!(list.warnings.len(), 0);
    assert_eq!(
        list.data,
        json!({
            "action": "site_page_list",
            "identity": identity_json(),
            "domain": "tenant.example",
            "pages": [{ "slug": "hello" }],
            "count": 1,
        })
    );

    for action in ["site_page_get", "site_page_create", "site_page_update"] {
        let page = site_page_action_result(action, &identity, json!({ "slug": "hello" }));
        assert_eq!(page.summary, "");
        assert_eq!(page.warnings.len(), 0);
        assert_eq!(
            page.data,
            json!({
                "action": action,
                "identity": identity_json(),
                "page": { "slug": "hello" },
            })
        );
    }

    let rename = site_page_rename_result(
        &identity,
        " old ",
        " new ",
        json!({ "slug": "new", "body": "Body" }),
    );
    assert_eq!(rename.summary, "");
    assert_eq!(
        rename.data,
        json!({
            "action": "site_page_rename",
            "identity": identity_json(),
            "from": "old",
            "to": "new",
            "page": { "slug": "new", "body": "Body" },
        })
    );

    let delete = site_page_delete_result(
        &identity,
        " Tenant.Example. ",
        " old ",
        json!({ "deleted": true }),
    );
    assert_eq!(delete.summary, "");
    assert_eq!(
        delete.data,
        json!({
            "action": "site_page_delete",
            "identity": identity_json(),
            "domain": "tenant.example",
            "slug": "old",
            "result": { "deleted": true },
        })
    );
}

fn identity_json() -> serde_json::Value {
    json!({
        "identity_name": "alice",
        "did": "did:wba:example.com:user:alice:e1",
        "handle": "alice",
    })
}

fn assert_error_contains(error: SiteError, needle: &str) {
    let message = error.to_string();
    assert!(
        message.contains(needle),
        "{message:?} should contain {needle:?}"
    );
}
