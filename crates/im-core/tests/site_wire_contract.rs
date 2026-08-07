use awiki_im_core::compat::site::{
    build_create_page_rpc_call, build_delete_page_rpc_call, build_get_page_rpc_call,
    build_get_root_rpc_call, build_list_pages_rpc_call, build_rename_page_rpc_call,
    build_set_root_rpc_call, build_update_page_rpc_call, TransportProfile, SITE_RPC_ENDPOINT,
};
use awiki_im_core::content::PageSlug;
use awiki_im_core::site::{
    SiteDomain, SitePageDraft, SitePageQuery, SitePageRef, SitePageUpdate, SiteRootDraft,
};
use serde_json::json;

#[test]
fn site_rpc_calls_match_go_methods_profiles_and_params() {
    assert_eq!(SITE_RPC_ENDPOINT, "/user-service/v1/site/rpc");

    let domain = SiteDomain::parse(" Tenant.Example. ").expect("domain");
    let get_root = build_get_root_rpc_call(domain.clone());
    assert_eq!(get_root.endpoint, SITE_RPC_ENDPOINT);
    assert_eq!(get_root.method, "get_root");
    assert_eq!(get_root.profile, TransportProfile::RpcReadHeavy);
    assert_eq!(get_root.params, json!({ "domain": "tenant.example" }));

    let set_root = build_set_root_rpc_call(SiteRootDraft {
        domain: domain.clone(),
        body: String::new(),
    });
    assert_eq!(set_root.method, "set_root");
    assert_eq!(set_root.profile, TransportProfile::RpcDefault);
    assert_eq!(
        set_root.params,
        json!({ "domain": "tenant.example", "body": "" })
    );

    let list_pages = build_list_pages_rpc_call(SitePageQuery {
        domain: domain.clone(),
        ..Default::default()
    });
    assert_eq!(list_pages.method, "list_pages");
    assert_eq!(list_pages.profile, TransportProfile::RpcReadHeavy);
    assert_eq!(list_pages.params, json!({ "domain": "tenant.example" }));

    let page = SitePageRef::new(domain.clone(), PageSlug::parse(" hello ").expect("slug"));
    let get_page = build_get_page_rpc_call(page.clone());
    assert_eq!(get_page.method, "get_page");
    assert_eq!(get_page.profile, TransportProfile::RpcReadHeavy);
    assert_eq!(
        get_page.params,
        json!({ "domain": "tenant.example", "slug": "hello" })
    );

    let create_page = build_create_page_rpc_call(SitePageDraft {
        domain: domain.clone(),
        slug: PageSlug::parse(" hello ").expect("slug"),
        body: String::new(),
    });
    assert_eq!(create_page.method, "create_page");
    assert_eq!(create_page.profile, TransportProfile::RpcDefault);
    assert_eq!(
        create_page.params,
        json!({ "domain": "tenant.example", "slug": "hello", "body": "" })
    );

    let update_page = build_update_page_rpc_call(
        page.clone(),
        SitePageUpdate {
            body: String::new(),
        },
    );
    assert_eq!(update_page.method, "update_page");
    assert_eq!(update_page.profile, TransportProfile::RpcDefault);
    assert_eq!(
        update_page.params,
        json!({ "domain": "tenant.example", "slug": "hello", "body": "" })
    );

    let rename_page =
        build_rename_page_rpc_call(page.clone(), PageSlug::parse(" new ").expect("target"));
    assert_eq!(rename_page.method, "rename_page");
    assert_eq!(rename_page.profile, TransportProfile::RpcDefault);
    assert_eq!(
        rename_page.params,
        json!({
            "domain": "tenant.example",
            "old_slug": "hello",
            "new_slug": "new",
        })
    );

    let delete_page = build_delete_page_rpc_call(page);
    assert_eq!(delete_page.method, "delete_page");
    assert_eq!(delete_page.profile, TransportProfile::RpcDefault);
    assert_eq!(
        delete_page.params,
        json!({ "domain": "tenant.example", "slug": "hello" })
    );
}

#[test]
fn site_wire_validation_matches_go_live_boundaries() {
    assert_eq!(
        SiteDomain::parse(" Tenant.Example. ")
            .expect("normalized live domain")
            .as_str(),
        "tenant.example"
    );
    assert_error_contains(
        SiteDomain::parse(" ").expect_err("missing domain"),
        "did_domain is required",
    );
    assert_error_contains(
        SiteDomain::parse("https://tenant.example").expect_err("URL-like domain"),
        "bare domain",
    );
    assert!(PageSlug::parse(" ").is_err());
}

#[test]
fn site_normalizers_return_typed_documents_with_raw_payloads() {
    let domain = SiteDomain::parse("tenant.example").expect("domain");
    let root = awiki_im_core::compat::site::normalize_root(
        json!({
            "domain": "tenant.example",
            "body": "# Home",
            "extra": true,
        }),
        &domain,
    )
    .expect("root");
    assert_eq!(root.domain.as_str(), "tenant.example");
    assert_eq!(root.body.as_deref(), Some("# Home"));
    assert_eq!(root.raw["extra"], true);

    let list = awiki_im_core::compat::site::normalize_page_list(
        &domain,
        json!({
            "count": 1,
            "pages": [{ "slug": "hello", "body": "Body" }],
        }),
    )
    .expect("list");
    assert_eq!(list.items.len(), 1);
    assert_eq!(list.items[0].domain.as_str(), "tenant.example");
    assert_eq!(list.items[0].slug.as_str(), "hello");
    assert_eq!(list.items[0].body.as_deref(), Some("Body"));
}

fn assert_error_contains(err: awiki_im_core::ImError, expected: &str) {
    let message = err.to_string();
    assert!(
        message.contains(expected),
        "error {message:?} should contain {expected:?}"
    );
}
