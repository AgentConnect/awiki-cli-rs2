use awiki_cli::authsdk::{HttpError, RpcError, JSON_RPC_ID, JSON_RPC_VERSION};
use awiki_cli::identity::types::{IdentitySummary, UserState};
use awiki_cli::identity::wire::{
    bind_email_completed_result, bind_email_pending_result, bind_email_sent_result,
    bind_phone_completed_result, bind_phone_otp_result, build_email_send_rest_call,
    build_email_status_rest_call, build_get_me_profile_rpc_call,
    build_handle_lookup_by_did_rpc_call, build_handle_lookup_by_handle_rpc_call,
    build_phone_bind_send_rest_call, build_phone_bind_verify_rest_call,
    build_profile_resolve_rpc_call, build_public_profile_rpc_call, build_recover_handle_rpc_call,
    build_refresh_token_rpc_call, build_register_rpc_call, build_replace_did_rpc_call,
    build_send_otp_rpc_call, build_update_me_profile_rpc_call, build_update_profile_payload,
    handle_lookup_error_is_not_found, normalize_email, normalize_handle_lookup_result,
    normalize_phone, profile_public_result, profile_self_result, profile_update_result,
    recover_otp_result, refresh_token_result, register_completed_result, register_phone_otp_result,
    registration_email_pending_result, registration_email_sent_result, replace_did_result,
    resolve_result, sanitize_otp, split_csv, HandleLookupResult, RecoverHandleRpcParams,
    RegisterRpcParams, ReplaceDidRpcParams, ServiceError, UpdateProfileParams,
    DID_AUTH_RPC_ENDPOINT, DID_PROFILE_RPC_ENDPOINT, EMAIL_SEND_ENDPOINT, EMAIL_STATUS_ENDPOINT,
    HANDLE_RPC_ENDPOINT, PHONE_BIND_SEND_ENDPOINT, PHONE_BIND_VERIFY_ENDPOINT,
};
use awiki_cli::transportcfg::Profile;
use serde_json::{json, Value};

#[test]
fn identity_wire_endpoint_constants_match_go_client() {
    assert_eq!(DID_AUTH_RPC_ENDPOINT, "/user-service/did-auth/rpc");
    assert_eq!(HANDLE_RPC_ENDPOINT, "/user-service/handle/rpc");
    assert_eq!(DID_PROFILE_RPC_ENDPOINT, "/user-service/did/profile/rpc");
    assert_eq!(EMAIL_SEND_ENDPOINT, "/user-service/auth/email-send");
    assert_eq!(EMAIL_STATUS_ENDPOINT, "/user-service/auth/email-status");
    assert_eq!(
        PHONE_BIND_SEND_ENDPOINT,
        "/user-service/auth/phone-bind-send"
    );
    assert_eq!(
        PHONE_BIND_VERIFY_ENDPOINT,
        "/user-service/auth/phone-bind-verify"
    );
}

#[test]
fn identity_service_error_display_and_conversions_match_go_client() {
    let rpc: ServiceError = RpcError {
        code: -32002,
        message: "not found".to_string(),
        data: Some(json!({ "kind": "handle" })),
    }
    .into();
    assert_eq!(rpc.to_string(), "service rpc error -32002: not found");
    assert_eq!(rpc.rpc_code, -32002);
    assert_eq!(rpc.data, Some(json!({ "kind": "handle" })));

    let http: ServiceError = HttpError {
        status_code: 404,
        message: "missing".to_string(),
    }
    .into();
    assert_eq!(http.to_string(), "service http error 404: missing");
    assert_eq!(http.status_code, 404);

    let plain = ServiceError {
        status_code: 0,
        rpc_code: 0,
        message: "plain".to_string(),
        data: None,
    };
    assert_eq!(plain.to_string(), "plain");
}

#[test]
fn identity_json_rpc_builders_match_go_methods_profiles_and_params() {
    let did = " did:wba:tenant.example:user:alice:e1 ";
    let lookup_did = build_handle_lookup_by_did_rpc_call(did).expect("lookup by did");
    assert_eq!(lookup_did.endpoint, HANDLE_RPC_ENDPOINT);
    assert_eq!(lookup_did.method, "lookup");
    assert_eq!(lookup_did.profile, Profile::RpcDefault);
    assert_eq!(
        lookup_did.params,
        json!({ "did": "did:wba:tenant.example:user:alice:e1" })
    );
    assert_eq!(
        lookup_did.payload(),
        json!({
            "jsonrpc": JSON_RPC_VERSION,
            "id": JSON_RPC_ID,
            "method": "lookup",
            "params": { "did": "did:wba:tenant.example:user:alice:e1" },
        })
    );

    let lookup_handle =
        build_handle_lookup_by_handle_rpc_call(" alice.tenant.example ").expect("lookup handle");
    assert_eq!(lookup_handle.endpoint, HANDLE_RPC_ENDPOINT);
    assert_eq!(lookup_handle.method, "lookup");
    assert_eq!(lookup_handle.profile, Profile::RpcDefault);
    assert_eq!(
        lookup_handle.params,
        json!({ "handle": "alice.tenant.example" })
    );

    let resolve = build_profile_resolve_rpc_call(did).expect("resolve did");
    assert_eq!(resolve.endpoint, DID_PROFILE_RPC_ENDPOINT);
    assert_eq!(resolve.method, "resolve");
    assert_eq!(resolve.profile, Profile::RpcDefault);
    assert_eq!(
        resolve.params,
        json!({ "did": "did:wba:tenant.example:user:alice:e1" })
    );

    let public_profile = build_public_profile_rpc_call(did).expect("public profile");
    assert_eq!(public_profile.endpoint, DID_PROFILE_RPC_ENDPOINT);
    assert_eq!(public_profile.method, "get_public_profile");
    assert_eq!(public_profile.profile, Profile::RpcReadHeavy);
    assert_eq!(
        public_profile.params,
        json!({ "did": "did:wba:tenant.example:user:alice:e1" })
    );

    let get_me = build_get_me_profile_rpc_call();
    assert_eq!(get_me.endpoint, DID_PROFILE_RPC_ENDPOINT);
    assert_eq!(get_me.method, "get_me");
    assert_eq!(get_me.profile, Profile::RpcReadHeavy);
    assert_eq!(get_me.params, json!({}));

    let refresh = build_refresh_token_rpc_call();
    assert_eq!(refresh.endpoint, DID_AUTH_RPC_ENDPOINT);
    assert_eq!(refresh.method, "get_me");
    assert_eq!(refresh.profile, Profile::AuthRefresh);
    assert_eq!(refresh.params, json!({}));

    let send_otp = build_send_otp_rpc_call("13800138000").expect("send otp");
    assert_eq!(send_otp.endpoint, HANDLE_RPC_ENDPOINT);
    assert_eq!(send_otp.method, "send_otp");
    assert_eq!(send_otp.profile, Profile::RpcDefault);
    assert_eq!(send_otp.params, json!({ "phone": "+8613800138000" }));
}

#[test]
fn identity_register_recover_and_replace_rpc_params_match_go_service() {
    let register = build_register_rpc_call(RegisterRpcParams {
        did_document: did_document(),
        handle: " alice ".to_string(),
        phone: Some("13800138000".to_string()),
        otp_code: Some(" 12 34 56 ".to_string()),
        email: None,
        invite_code: "invite-1".to_string(),
    })
    .expect("register phone");
    assert_eq!(register.endpoint, DID_AUTH_RPC_ENDPOINT);
    assert_eq!(register.method, "register");
    assert_eq!(register.profile, Profile::RpcDefault);
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

    let register_email = build_register_rpc_call(RegisterRpcParams {
        did_document: did_document(),
        handle: "alice".to_string(),
        email: Some(" Alice@Example.COM ".to_string()),
        ..Default::default()
    })
    .expect("register email");
    assert_eq!(
        register_email.params,
        json!({
            "did_document": did_document(),
            "handle": "alice",
            "email": "alice@example.com",
        })
    );

    let recover = build_recover_handle_rpc_call(RecoverHandleRpcParams {
        did_document: did_document(),
        handle: " alice.tenant.example ".to_string(),
        phone: "+15551234567".to_string(),
        otp_code: " 65 43 21 ".to_string(),
    })
    .expect("recover handle");
    assert_eq!(recover.endpoint, DID_AUTH_RPC_ENDPOINT);
    assert_eq!(recover.method, "recover_handle");
    assert_eq!(recover.profile, Profile::RpcDefault);
    assert_eq!(
        recover.params,
        json!({
            "did_document": did_document(),
            "handle": "alice.tenant.example",
            "phone": "+15551234567",
            "otp_code": "654321",
        })
    );

    let replace = build_replace_did_rpc_call(ReplaceDidRpcParams {
        new_did_document: did_document(),
        is_public: Some(false),
        is_agent: Some(true),
        role: Some(" ".to_string()),
        endpoint_url: Some(" https://example.com/agent ".to_string()),
    });
    assert_eq!(replace.endpoint, DID_AUTH_RPC_ENDPOINT);
    assert_eq!(replace.method, "replace_did");
    assert_eq!(replace.profile, Profile::RpcDefault);
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
        build_email_status_rest_call(" Alice@Example.COM ", Some("alice.tenant.example"), false)
            .expect("email status");
    assert_eq!(email_status.endpoint, EMAIL_STATUS_ENDPOINT);
    assert_eq!(email_status.method, "GET");
    assert_eq!(email_status.profile, Profile::RpcDefault);
    assert_eq!(email_status.authenticated, false);
    assert_eq!(email_status.body, Value::Null);
    assert_eq!(
        email_status.query.get("email").map(String::as_str),
        Some("alice@example.com")
    );
    assert_eq!(
        email_status.query.get("handle").map(String::as_str),
        Some("alice.tenant.example")
    );

    let bind_email_status =
        build_email_status_rest_call(" Alice@Example.COM ", None, true).expect("bind status");
    assert_eq!(bind_email_status.authenticated, true);
    assert_eq!(bind_email_status.query.len(), 1);
    assert_eq!(
        bind_email_status.query.get("email").map(String::as_str),
        Some("alice@example.com")
    );

    let email_send =
        build_email_send_rest_call(" Alice@Example.COM ", Some("alice.tenant.example"), false)
            .expect("registration email send");
    assert_eq!(email_send.endpoint, EMAIL_SEND_ENDPOINT);
    assert_eq!(email_send.method, "POST");
    assert_eq!(email_send.authenticated, false);
    assert_eq!(
        email_send.body,
        json!({ "email": "alice@example.com", "handle": "alice.tenant.example" })
    );

    let bind_email_send =
        build_email_send_rest_call(" Alice@Example.COM ", None, true).expect("bind email send");
    assert_eq!(bind_email_send.authenticated, true);
    assert_eq!(
        bind_email_send.body,
        json!({ "email": "alice@example.com" })
    );

    let phone_send = build_phone_bind_send_rest_call("13800138000").expect("phone send");
    assert_eq!(phone_send.endpoint, PHONE_BIND_SEND_ENDPOINT);
    assert_eq!(phone_send.method, "POST");
    assert_eq!(phone_send.authenticated, true);
    assert_eq!(phone_send.body, json!({ "phone": "+8613800138000" }));

    let phone_verify =
        build_phone_bind_verify_rest_call("13800138000", " 12 34 56 ").expect("phone verify");
    assert_eq!(phone_verify.endpoint, PHONE_BIND_VERIFY_ENDPOINT);
    assert_eq!(phone_verify.method, "POST");
    assert_eq!(phone_verify.authenticated, true);
    assert_eq!(
        phone_verify.body,
        json!({ "phone": "+8613800138000", "code": "123456" })
    );
}

#[test]
fn identity_helper_normalization_matches_go_service() {
    assert_eq!(normalize_phone("+15551234567").unwrap(), "+15551234567");
    assert_eq!(normalize_phone("13800138000").unwrap(), "+8613800138000");
    assert!(normalize_phone("555")
        .unwrap_err()
        .to_string()
        .contains("invalid input: invalid phone number \"555\""));
    assert_eq!(sanitize_otp(" 12 34 56 "), "123456");
    assert_eq!(
        split_csv(" rust, port, ,cli "),
        vec!["rust".to_string(), "port".to_string(), "cli".to_string()]
    );
    assert_eq!(normalize_email("  Alice@Example.COM "), "alice@example.com");
}

#[test]
fn identity_profile_update_payload_matches_go_mapping_and_order() {
    let (payload, changed_fields) = build_update_profile_payload(UpdateProfileParams {
        display_name: " Alice ".to_string(),
        bio: " Rust port ".to_string(),
        tags_csv: " rust, port, ,cli ".to_string(),
        markdown: " # Profile ".to_string(),
        preserve_markdown: false,
    })
    .expect("profile payload");
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

    let call = build_update_me_profile_rpc_call(UpdateProfileParams {
        display_name: " Alice ".to_string(),
        ..Default::default()
    })
    .expect("update profile call");
    assert_eq!(call.call.endpoint, DID_PROFILE_RPC_ENDPOINT);
    assert_eq!(call.call.method, "update_me");
    assert_eq!(call.call.profile, Profile::RpcDefault);
    assert_eq!(call.call.params, json!({ "nick_name": "Alice" }));
    assert_eq!(call.changed_fields, vec!["display_name".to_string()]);

    assert!(build_update_profile_payload(UpdateProfileParams::default())
        .unwrap_err()
        .to_string()
        .contains("invalid input: no profile fields were provided"));
}

#[test]
fn identity_handle_lookup_result_interpretation_matches_go_client() {
    assert!(build_handle_lookup_by_did_rpc_call(" ")
        .unwrap_err()
        .to_string()
        .contains("invalid input: did is required"));

    assert!(handle_lookup_error_is_not_found(&ServiceError {
        status_code: 404,
        rpc_code: 0,
        message: "not found".to_string(),
        data: None,
    }));
    assert!(handle_lookup_error_is_not_found(&ServiceError {
        status_code: 0,
        rpc_code: -32002,
        message: "not found".to_string(),
        data: None,
    }));

    assert_eq!(
        normalize_handle_lookup_result(HandleLookupResult {
            handle: " ".to_string(),
            did: "did:wba:tenant.example:user:alice:e1".to_string(),
            ..Default::default()
        }),
        None
    );
    assert_eq!(
        normalize_handle_lookup_result(HandleLookupResult {
            handle: "alice".to_string(),
            did: " ".to_string(),
            ..Default::default()
        }),
        None
    );
    assert_eq!(
        normalize_handle_lookup_result(HandleLookupResult {
            handle: "alice".to_string(),
            did: "did:wba:tenant.example:user:alice:e1".to_string(),
            domain: "tenant.example".to_string(),
            full_handle: "alice.tenant.example".to_string(),
            status: "active".to_string(),
        }),
        Some(HandleLookupResult {
            handle: "alice".to_string(),
            did: "did:wba:tenant.example:user:alice:e1".to_string(),
            domain: "tenant.example".to_string(),
            full_handle: "alice.tenant.example".to_string(),
            status: "active".to_string(),
        })
    );
}

#[test]
fn identity_registration_and_recover_result_shapes_match_go_service() {
    let otp = register_phone_otp_result(
        "alice",
        "alice",
        "alice.tenant.example",
        "13800138000",
        json!({ "sent": true }),
    )
    .expect("otp result");
    assert_eq!(otp.summary, "OTP sent for handle alice.tenant.example");
    assert_eq!(
        otp.data,
        json!({
            "action": "send_handle_otp",
            "identity_name": "alice",
            "handle": "alice",
            "full_handle": "alice.tenant.example",
            "method": "phone",
            "phone": "+8613800138000",
            "verification_state": "otp_sent",
            "result": { "sent": true },
        })
    );

    let email = registration_email_sent_result(
        "alice",
        "alice",
        "alice.tenant.example",
        " Alice@Example.COM ",
        json!({ "sent": true }),
    );
    assert_eq!(
        email.summary,
        "Activation email sent for handle alice.tenant.example"
    );
    assert_eq!(
        email.data,
        json!({
            "action": "send_registration_email",
            "identity_name": "alice",
            "handle": "alice",
            "full_handle": "alice.tenant.example",
            "method": "email",
            "email": "alice@example.com",
            "verification_state": "email_sent",
            "result": { "sent": true },
        })
    );

    let pending =
        registration_email_pending_result("alice", "alice", "alice.tenant.example", "a@b.test");
    assert_eq!(pending.summary, "Email verification is still pending");
    assert_eq!(pending.data["action"], "wait_for_registration_email");
    assert_eq!(pending.data["verification_state"], "pending");

    let completed = register_completed_result(
        &identity(),
        "alice.tenant.example",
        "email",
        json!({ "did": "did:wba:tenant.example:user:alice:e1" }),
    );
    assert_eq!(
        completed.summary,
        "Handle alice.tenant.example registered successfully"
    );
    assert_eq!(completed.data["action"], "register_handle");
    assert_eq!(completed.data["identity"], identity_json());
    assert_eq!(completed.data["method"], "email");
    assert_eq!(completed.data["verification_state"], "completed");

    let recover = recover_otp_result(
        "alice",
        "alice",
        "alice.tenant.example",
        "13800138000",
        json!({ "sent": true }),
    )
    .expect("recover otp");
    assert_eq!(
        recover.summary,
        "OTP sent for handle alice.tenant.example recovery"
    );
    assert_eq!(recover.data["action"], "send_recover_otp");
    assert_eq!(recover.data["phone"], "+8613800138000");
}

#[test]
fn identity_bind_refresh_profile_and_replace_result_shapes_match_go_service() {
    let bind_phone_otp = bind_phone_otp_result(&identity(), "13800138000", json!({ "sent": true }))
        .expect("phone otp");
    assert_eq!(bind_phone_otp.summary, "Phone binding OTP sent");
    assert_eq!(bind_phone_otp.data["action"], "send_bind_phone_otp");
    assert_eq!(bind_phone_otp.data["identity"], identity_json());
    assert_eq!(bind_phone_otp.data["phone"], "+8613800138000");

    let bind_phone =
        bind_phone_completed_result(&identity(), "13800138000", json!({ "bound": true }))
            .expect("phone complete");
    assert_eq!(bind_phone.summary, "Phone bound successfully");
    assert_eq!(bind_phone.data["action"], "bind_phone");
    assert_eq!(bind_phone.data["verification_state"], "completed");

    let bind_email = bind_email_sent_result(&identity(), " Alice@Example.COM ", json!({}));
    assert_eq!(bind_email.summary, "Binding email sent");
    assert_eq!(bind_email.data["action"], "send_bind_email");
    assert_eq!(bind_email.data["email"], "alice@example.com");

    let bind_pending = bind_email_pending_result(&identity(), " Alice@Example.COM ");
    assert_eq!(bind_pending.summary, "Email verification is still pending");
    assert_eq!(bind_pending.data["action"], "wait_for_bind_email");
    assert_eq!(bind_pending.data["verification_state"], "pending");

    let bind_done = bind_email_completed_result(&identity(), " Alice@Example.COM ");
    assert_eq!(bind_done.summary, "Email binding verified successfully");
    assert_eq!(bind_done.data["action"], "bind_email");
    assert_eq!(bind_done.data["verification_state"], "completed");

    let refresh = refresh_token_result(&identity(), true);
    assert_eq!(refresh.summary, "JWT refreshed for identity alice");
    assert_eq!(refresh.data["action"], "refresh_token");
    assert_eq!(refresh.data["previous_token_present"], true);
    assert_eq!(
        refresh.data["auth_flow"],
        "did_auth_get_me_without_stored_bearer"
    );

    let self_profile = profile_self_result(json!({ "nick_name": "Alice" }));
    assert_eq!(self_profile.summary, "Fetched current identity profile");
    assert_eq!(
        self_profile.data,
        json!({ "subject": "self", "profile": { "nick_name": "Alice" } })
    );

    let public_profile = profile_public_result(
        json!({
            "handle": "alice",
            "full_handle": "alice.tenant.example",
            "domain": "tenant.example",
            "did": "did:wba:tenant.example:user:alice:e1",
        }),
        json!({ "nick_name": "Alice" }),
    );
    assert_eq!(public_profile.summary, "Fetched public profile");
    assert_eq!(
        public_profile.data["profile"],
        json!({ "nick_name": "Alice" })
    );

    let update = profile_update_result(
        &identity(),
        vec!["display_name".to_string(), "bio".to_string()],
        json!({ "nick_name": "Alice" }),
    );
    assert_eq!(update.summary, "Profile updated successfully");
    assert_eq!(update.data["action"], "update_profile");
    assert_eq!(update.data["identity"], identity_json());
    assert_eq!(
        update.data["changed_fields"],
        json!(["display_name", "bio"])
    );

    let replace = replace_did_result(
        &identity(),
        "did:wba:tenant.example:user:alice:old",
        "did:wba:tenant.example:user:alice:new",
        "/tmp/backup",
        json!({ "ok": true }),
    );
    assert_eq!(replace.summary, "Identity alice DID replaced successfully");
    assert_eq!(replace.data["action"], "replace_did");
    assert_eq!(
        replace.data["old_did"],
        "did:wba:tenant.example:user:alice:old"
    );
    assert_eq!(replace.data["did"], "did:wba:tenant.example:user:alice:new");
    assert_eq!(replace.data["backup_path"], "/tmp/backup");
}

#[test]
fn identity_resolve_result_preserves_optional_data_and_warnings() {
    let result = resolve_result(
        Some(json!({ "did": "did:wba:tenant.example:user:alice:e1" })),
        Some(json!({ "handle": "alice" })),
        Some(json!({ "nick_name": "Alice" })),
        vec!["Public profile lookup failed: service unavailable".to_string()],
    );
    assert_eq!(result.summary, "Identity resolved successfully");
    assert_eq!(
        result.data,
        json!({
            "resolve": { "did": "did:wba:tenant.example:user:alice:e1" },
            "lookup": { "handle": "alice" },
            "public_profile": { "nick_name": "Alice" },
        })
    );
    assert_eq!(
        result.warnings,
        vec!["Public profile lookup failed: service unavailable".to_string()]
    );

    let did_only = resolve_result(
        Some(json!({ "did": "did:example" })),
        None,
        None,
        Vec::new(),
    );
    assert_eq!(
        did_only.data,
        json!({ "resolve": { "did": "did:example" } })
    );
    assert!(did_only.warnings.is_empty());
}

fn did_document() -> Value {
    json!({
        "id": "did:wba:tenant.example:user:alice:e1",
        "service": [],
    })
}

fn identity() -> IdentitySummary {
    IdentitySummary {
        identity_name: "alice".to_string(),
        did: "did:wba:tenant.example:user:alice:e1".to_string(),
        unique_id: "e1".to_string(),
        display_name: "Alice".to_string(),
        handle: "alice".to_string(),
        full_handle: "alice.tenant.example".to_string(),
        created_at: "2026-05-15T00:00:00Z".to_string(),
        dir_name: "alice".to_string(),
        is_default: true,
        has_jwt: true,
        has_did_document: true,
        has_key1_private: true,
        has_key1_public: true,
        has_e2ee_signing_private: true,
        has_e2ee_agreement_private: true,
        user_state: UserState {
            registration_state: "registered".to_string(),
            ready_for_messaging: true,
            missing: Vec::new(),
        },
        user_id: "user-1".to_string(),
    }
}

fn identity_json() -> Value {
    json!({
        "identity_name": "alice",
        "did": "did:wba:tenant.example:user:alice:e1",
        "unique_id": "e1",
        "display_name": "Alice",
        "handle": "alice",
        "full_handle": "alice.tenant.example",
        "created_at": "2026-05-15T00:00:00Z",
        "dir_name": "alice",
        "is_default": true,
        "has_jwt": true,
        "has_did_document": true,
        "has_key1_private": true,
        "has_key1_public": true,
        "has_e2ee_signing_private": true,
        "has_e2ee_agreement_private": true,
        "user_state": {
            "registration_state": "registered",
            "ready_for_messaging": true,
        },
    })
}
