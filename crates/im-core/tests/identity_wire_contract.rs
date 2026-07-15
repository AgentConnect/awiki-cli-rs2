use awiki_im_core::compat::{directory, identity};
use awiki_im_core::prelude::{Did, Handle, Profile, ProfileAttribute};
use serde_json::{json, Value};

#[test]
fn identity_wire_endpoint_constants_match_go_client() {
    assert_eq!(
        identity::DID_AUTH_RPC_ENDPOINT,
        "/user-service/did-auth/rpc"
    );
    assert_eq!(directory::HANDLE_RPC_ENDPOINT, "/user-service/handle/rpc");
    assert_eq!(
        directory::DID_RELATIONSHIPS_RPC_ENDPOINT,
        "/user-service/did/relationships/rpc"
    );
    assert_eq!(
        identity::DID_PROFILE_RPC_ENDPOINT,
        "/user-service/did/profile/rpc"
    );
    assert_eq!(
        identity::EMAIL_SEND_ENDPOINT,
        "/user-service/auth/email-send"
    );
    assert_eq!(
        identity::EMAIL_STATUS_ENDPOINT,
        "/user-service/auth/email-status"
    );
    assert_eq!(
        identity::PHONE_BIND_SEND_ENDPOINT,
        "/user-service/auth/phone-bind-send"
    );
    assert_eq!(
        identity::PHONE_BIND_VERIFY_ENDPOINT,
        "/user-service/auth/phone-bind-verify"
    );
}

#[test]
fn identity_relationship_rpc_builders_match_go_methods_profiles_and_params() {
    let did = " did:wba:tenant.example:user:bob:e1 ";
    let follow = directory::build_follow_rpc_call(did).unwrap();
    assert_eq!(follow.endpoint, directory::DID_RELATIONSHIPS_RPC_ENDPOINT);
    assert_eq!(follow.method, "follow");
    assert_eq!(follow.profile, directory::TransportProfile::RpcDefault);
    assert_eq!(
        follow.params,
        json!({ "target_did": "did:wba:tenant.example:user:bob:e1" })
    );

    let unfollow = directory::build_unfollow_rpc_call(did).unwrap();
    assert_eq!(unfollow.endpoint, directory::DID_RELATIONSHIPS_RPC_ENDPOINT);
    assert_eq!(unfollow.method, "unfollow");
    assert_eq!(unfollow.profile, directory::TransportProfile::RpcDefault);
    assert_eq!(
        unfollow.params,
        json!({ "target_did": "did:wba:tenant.example:user:bob:e1" })
    );

    let status = directory::build_relationship_status_rpc_call(did).unwrap();
    assert_eq!(status.endpoint, directory::DID_RELATIONSHIPS_RPC_ENDPOINT);
    assert_eq!(status.method, "get_status");
    assert_eq!(status.profile, directory::TransportProfile::RpcDefault);
    assert_eq!(
        status.params,
        json!({ "target_did": "did:wba:tenant.example:user:bob:e1" })
    );

    let followers = directory::build_followers_rpc_call(25, 10).unwrap();
    assert_eq!(
        followers.endpoint,
        directory::DID_RELATIONSHIPS_RPC_ENDPOINT
    );
    assert_eq!(followers.method, "get_followers");
    assert_eq!(followers.profile, directory::TransportProfile::RpcReadHeavy);
    assert_eq!(followers.params, json!({ "limit": 25, "offset": 10 }));

    let following = directory::build_following_rpc_call(50, 0).unwrap();
    assert_eq!(
        following.endpoint,
        directory::DID_RELATIONSHIPS_RPC_ENDPOINT
    );
    assert_eq!(following.method, "get_following");
    assert_eq!(following.profile, directory::TransportProfile::RpcReadHeavy);
    assert_eq!(following.params, json!({ "limit": 50, "offset": 0 }));

    assert!(matches!(
        directory::build_follow_rpc_call(" "),
        Err(awiki_im_core::ImError::InvalidInput { field: Some(field), .. }) if field == "target_did"
    ));
    assert!(matches!(
        directory::build_followers_rpc_call(0, 0),
        Err(awiki_im_core::ImError::InvalidInput { field: Some(field), .. }) if field == "limit"
    ));
}

#[test]
fn identity_json_rpc_builders_match_go_methods_profiles_and_params() {
    let did = " did:wba:tenant.example:user:alice:e1 ";
    let lookup_did = directory::build_handle_lookup_by_did_rpc_call(did).unwrap();
    assert_eq!(lookup_did.endpoint, directory::HANDLE_RPC_ENDPOINT);
    assert_eq!(lookup_did.method, "lookup");
    assert_eq!(lookup_did.profile, directory::TransportProfile::RpcDefault);
    assert_eq!(
        lookup_did.params,
        json!({ "did": "did:wba:tenant.example:user:alice:e1" })
    );
    assert_eq!(
        lookup_did.payload(),
        json!({
            "jsonrpc": "2.0",
            "id": "req-1",
            "method": "lookup",
            "params": { "did": "did:wba:tenant.example:user:alice:e1" },
        })
    );

    let lookup_handle = directory::build_handle_lookup_by_handle_rpc_call(" alice.tenant.example ")
        .expect("lookup handle");
    assert_eq!(lookup_handle.endpoint, directory::HANDLE_RPC_ENDPOINT);
    assert_eq!(lookup_handle.method, "lookup");
    assert_eq!(
        lookup_handle.params,
        json!({ "handle": "alice.tenant.example" })
    );

    let resolve = directory::build_profile_resolve_rpc_call(did).unwrap();
    assert_eq!(resolve.endpoint, identity::DID_PROFILE_RPC_ENDPOINT);
    assert_eq!(resolve.method, "resolve");
    assert_eq!(resolve.profile, directory::TransportProfile::RpcDefault);
    assert_eq!(
        resolve.params,
        json!({ "did": "did:wba:tenant.example:user:alice:e1" })
    );

    let public_profile = directory::build_public_profile_rpc_call(did).unwrap();
    assert_eq!(public_profile.endpoint, identity::DID_PROFILE_RPC_ENDPOINT);
    assert_eq!(public_profile.method, "get_public_profile");
    assert_eq!(
        public_profile.profile,
        directory::TransportProfile::RpcReadHeavy
    );

    let get_me = identity::build_get_me_profile_rpc_call();
    assert_eq!(get_me.endpoint, identity::DID_PROFILE_RPC_ENDPOINT);
    assert_eq!(get_me.method, "get_me");
    assert_eq!(get_me.profile, identity::TransportProfile::RpcReadHeavy);
    assert_eq!(get_me.params, json!({}));

    let refresh = identity::build_refresh_token_rpc_call();
    assert_eq!(refresh.endpoint, identity::DID_AUTH_RPC_ENDPOINT);
    assert_eq!(refresh.method, "get_me");
    assert_eq!(refresh.profile, identity::TransportProfile::AuthRefresh);
    assert_eq!(refresh.params, json!({}));

    let send_otp = directory::build_send_otp_rpc_call("13800138000").unwrap();
    assert_eq!(send_otp.endpoint, directory::HANDLE_RPC_ENDPOINT);
    assert_eq!(send_otp.method, "send_otp");
    assert_eq!(send_otp.params, json!({ "phone": "+8613800138000" }));
}

#[test]
fn identity_register_recover_and_replace_rpc_params_match_go_service() {
    let register = identity::build_register_rpc_call(identity::RegisterRpcParams {
        did_document: did_document(),
        handle: " alice ".to_string(),
        phone: Some("13800138000".to_string()),
        otp_code: Some(" 12 34 56 ".to_string()),
        email: None,
        invite_code: "invite-1".to_string(),
    })
    .unwrap();
    assert_eq!(register.endpoint, identity::DID_AUTH_RPC_ENDPOINT);
    assert_eq!(register.method, "register");
    assert_eq!(
        register.params,
        json!({
            "did_document": did_document(),
            "handle": "alice",
            "phone": "+8613800138000",
            "otp_code": "123456",
            "invite_code": "invite-1",
        })
    );

    let register_email = identity::build_register_rpc_call(identity::RegisterRpcParams {
        did_document: did_document(),
        handle: "alice".to_string(),
        email: Some(" Alice@Example.COM ".to_string()),
        ..identity::RegisterRpcParams::default()
    })
    .unwrap();
    assert_eq!(
        register_email.params,
        json!({
            "did_document": did_document(),
            "handle": "alice",
            "email": "alice@example.com",
        })
    );

    let recover = identity::build_recover_handle_rpc_call(identity::RecoverHandleRpcParams {
        did_document: did_document(),
        handle: " alice.tenant.example ".to_string(),
        phone: "+15551234567".to_string(),
        otp_code: " 65 43 21 ".to_string(),
    })
    .unwrap();
    assert_eq!(recover.endpoint, identity::DID_AUTH_RPC_ENDPOINT);
    assert_eq!(recover.method, "recover_handle");
    assert_eq!(
        recover.params,
        json!({
            "did_document": did_document(),
            "handle": "alice.tenant.example",
            "phone": "+15551234567",
            "otp_code": "654321",
        })
    );

    let update = identity::build_update_document_rpc_call(identity::UpdateDocumentRpcParams {
        did_document: did_document(),
        ..identity::UpdateDocumentRpcParams::default()
    });
    assert_eq!(update.endpoint, identity::DID_AUTH_RPC_ENDPOINT);
    assert_eq!(update.method, "update_document");
    assert_eq!(update.params, json!({ "did_document": did_document() }));

    let update_metadata =
        identity::build_update_document_rpc_call(identity::UpdateDocumentRpcParams {
            did_document: did_document(),
            is_public: Some(false),
            is_agent: Some(true),
            role: Some(" ".to_string()),
            endpoint_url: Some(" https://example.com/agent ".to_string()),
        });
    assert_eq!(
        update_metadata.params,
        json!({
            "did_document": did_document(),
            "is_public": false,
            "is_agent": true,
            "role": Value::Null,
            "endpoint_url": "https://example.com/agent",
        })
    );

    let replace = identity::build_replace_did_rpc_call(identity::ReplaceDidRpcParams {
        new_did_document: did_document(),
        is_public: Some(false),
        is_agent: Some(true),
        role: Some(" ".to_string()),
        endpoint_url: Some(" https://example.com/agent ".to_string()),
    });
    assert_eq!(replace.endpoint, identity::DID_AUTH_RPC_ENDPOINT);
    assert_eq!(replace.method, "replace_did");
    assert_eq!(
        replace.params,
        json!({
            "new_did_document": did_document(),
            "is_public": false,
            "is_agent": true,
            "role": Value::Null,
            "endpoint_url": "https://example.com/agent",
        })
    );
}

#[test]
fn identity_rest_builders_match_go_email_and_phone_bind_contracts() {
    let email_status =
        identity::build_email_status_rest_call(" Alice@Example.COM ", Some("alice.test"), false)
            .unwrap();
    assert_eq!(email_status.endpoint, identity::EMAIL_STATUS_ENDPOINT);
    assert_eq!(email_status.method, "GET");
    assert_eq!(email_status.profile, identity::TransportProfile::RpcDefault);
    assert!(!email_status.authenticated);
    assert_eq!(email_status.body, Value::Null);
    assert_eq!(
        email_status.query.get("email").map(String::as_str),
        Some("alice@example.com")
    );
    assert_eq!(
        email_status.query.get("handle").map(String::as_str),
        Some("alice.test")
    );

    let email_send =
        identity::build_email_send_rest_call(" Alice@Example.COM ", Some("alice.test"), false)
            .unwrap();
    assert_eq!(email_send.endpoint, identity::EMAIL_SEND_ENDPOINT);
    assert_eq!(email_send.method, "POST");
    assert_eq!(
        email_send.body,
        json!({ "email": "alice@example.com", "handle": "alice.test" })
    );

    let bind_email_send =
        identity::build_email_send_rest_call(" Alice@Example.COM ", None, true).unwrap();
    assert!(bind_email_send.authenticated);
    assert_eq!(
        bind_email_send.body,
        json!({ "email": "alice@example.com" })
    );

    let phone_send = identity::build_phone_bind_send_rest_call("13800138000").unwrap();
    assert_eq!(phone_send.endpoint, identity::PHONE_BIND_SEND_ENDPOINT);
    assert!(phone_send.authenticated);
    assert_eq!(phone_send.body, json!({ "phone": "+8613800138000" }));

    let phone_verify =
        identity::build_phone_bind_verify_rest_call("13800138000", " 12 34 56 ").unwrap();
    assert_eq!(phone_verify.endpoint, identity::PHONE_BIND_VERIFY_ENDPOINT);
    assert_eq!(
        phone_verify.body,
        json!({ "phone": "+8613800138000", "code": "123456" })
    );
}

#[test]
fn identity_helper_normalization_matches_go_service() {
    assert_eq!(
        identity::normalize_phone("+15551234567").unwrap(),
        "+15551234567"
    );
    assert_eq!(
        identity::normalize_phone("13800138000").unwrap(),
        "+8613800138000"
    );
    assert!(identity::normalize_phone("555")
        .unwrap_err()
        .to_string()
        .contains("invalid phone number \"555\""));
    assert_eq!(identity::sanitize_otp(" 12 34 56 "), "123456");
    assert_eq!(
        identity::split_csv(" rust, port, ,cli "),
        vec!["rust".to_string(), "port".to_string(), "cli".to_string()]
    );
    assert_eq!(
        identity::normalize_email("  Alice@Example.COM "),
        "alice@example.com"
    );
}

#[test]
fn identity_profile_update_payload_matches_go_mapping_and_order() {
    let (payload, changed_fields) =
        identity::build_update_profile_payload(identity::UpdateProfileParams {
            display_name: " Alice ".to_string(),
            bio: " Rust port ".to_string(),
            tags_csv: " rust, port, ,cli ".to_string(),
            markdown: " # Profile ".to_string(),
            avatar_uri: String::new(),
            avatar_url: String::new(),
            preserve_markdown: false,
        })
        .unwrap();
    assert_eq!(
        changed_fields,
        vec![
            "display_name".to_string(),
            "bio".to_string(),
            "tags".to_string(),
            "profile_md".to_string(),
        ]
    );
    assert_eq!(
        payload,
        json!({
            "nick_name": "Alice",
            "bio": "Rust port",
            "tags": ["rust", "port", "cli"],
            "profile_md": "# Profile",
        })
    );

    let call = identity::build_update_me_profile_rpc_call(identity::UpdateProfileParams {
        display_name: " Alice ".to_string(),
        ..identity::UpdateProfileParams::default()
    })
    .unwrap();
    assert_eq!(call.call.endpoint, identity::DID_PROFILE_RPC_ENDPOINT);
    assert_eq!(call.call.method, "update_me");
    assert_eq!(call.call.params, json!({ "nick_name": "Alice" }));
    assert_eq!(call.changed_fields, vec!["display_name".to_string()]);

    let (avatar_payload, avatar_changed_fields) =
        identity::build_update_profile_payload(identity::UpdateProfileParams {
            avatar_uri: " https://cdn.test/alice.png ".to_string(),
            ..identity::UpdateProfileParams::default()
        })
        .unwrap();
    assert_eq!(
        avatar_payload,
        json!({ "avatar_url": "https://cdn.test/alice.png" })
    );
    assert_eq!(avatar_changed_fields, vec!["avatar_uri".to_string()]);

    assert!(
        identity::build_update_profile_payload(identity::UpdateProfileParams::default())
            .unwrap_err()
            .to_string()
            .contains("no profile fields were provided")
    );
}

#[test]
fn identity_profile_wire_view_matches_cli_compat_shape() {
    let mut profile = Profile::new(Did::parse("did:example:alice").unwrap());
    profile.handle = Some(Handle::parse("alice.awiki.test", "").unwrap());
    profile.display_name = Some("Alice".to_string());
    profile.bio = Some("Rust port".to_string());
    profile.tags = vec!["rust".to_string(), "cli".to_string()];
    profile.markdown = Some("# Profile".to_string());
    profile.avatar_uri = Some("https://example.test/avatar.png".to_string());
    profile.avatar_url = Some("https://example.test/avatar.png".to_string());
    profile.profile_uri = Some("https://alice.awiki.test/".to_string());
    profile.subject_type = Some("person".to_string());
    profile.updated_at = Some("2026-05-25T00:00:00Z".to_string());
    profile.metadata = vec![ProfileAttribute {
        key: "source".to_string(),
        value: "profile".to_string(),
    }];

    assert_eq!(
        profile.to_wire_profile_value(),
        json!({
            "did": "did:example:alice",
            "subject_did": "did:example:alice",
            "handle": "alice.awiki.test",
            "display_name": "Alice",
            "nick_name": "Alice",
            "description": "Rust port",
            "bio": "Rust port",
            "tags": ["rust", "cli"],
            "profile_md": "# Profile",
            "avatar_uri": "https://example.test/avatar.png",
            "avatar_url": "https://example.test/avatar.png",
            "profile_uri": "https://alice.awiki.test/",
            "subject_type": "person",
            "updated": "2026-05-25T00:00:00Z",
            "updated_at": "2026-05-25T00:00:00Z",
            "metadata": {
                "source": "profile",
            },
        })
    );
}

fn did_document() -> Value {
    json!({
        "id": "did:wba:tenant.example:user:alice:e1",
        "service": [],
    })
}
