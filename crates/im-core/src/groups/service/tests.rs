use serde_json::json;
use std::path::PathBuf;

struct Fixture {
    root: PathBuf,
    did_bundle: anp::authentication::DidDocumentBundle,
}

impl Fixture {
    fn new() -> Self {
        let root = unique_temp_root();
        std::fs::create_dir_all(root.join("identities").join("alice")).unwrap();
        Self {
            root,
            did_bundle: test_did_bundle(),
        }
    }

    fn client_with_group_v2(&self, enabled: bool) -> crate::core::ImClient {
        crate::core::ImCore::new_with_options(
            crate::ImCoreConfig {
                service_base_url: crate::ServiceEndpoint::parse("https://example.test").unwrap(),
                did_domain: "example.test".to_owned(),
                client_version_info: None,
                user_service_endpoint: None,
                message_service_endpoint: None,
                mail_service_endpoint: None,
                anp_service_endpoint: None,
                anp_service_did: Some(crate::ids::Did::parse("did:example:service").unwrap()),
                ca_bundle: None,
                transport_policy: crate::MessageTransportPolicy::HttpOnly,
            },
            crate::ImCorePaths {
                identities: crate::paths::IdentityRegistryPaths {
                    identity_root_dir: self.root.join("identities"),
                    registry_path: self.root.join("identities").join("registry.json"),
                    default_identity_path: Some(self.root.join("identities").join("default")),
                },
                local_state: crate::paths::LocalStatePaths {
                    sqlite_path: self.root.join("local").join("im.sqlite"),
                },
                runtime: crate::paths::RuntimePaths {
                    cache_dir: self.root.join("cache"),
                    temp_dir: self.root.join("tmp"),
                },
            },
            crate::ImCoreOpenOptions::default().with_multi_device_group_e2ee_enabled(enabled),
        )
        .unwrap()
        .client(crate::identity::IdentitySelector::Did(
            crate::ids::Did::parse(self.did_bundle.did().unwrap()).unwrap(),
        ))
        .unwrap()
    }
}

#[test]
fn public_group_lifecycle_requires_v2_instead_of_falling_back() {
    let fixture = Fixture::new();
    let legacy = fixture.client_with_group_v2(false);
    let v2 = fixture.client_with_group_v2(true);

    assert!(matches!(
        super::require_group_e2ee_v2(&legacy),
        Err(crate::ImError::UnsupportedCapability { .. })
    ));
    assert!(super::require_group_e2ee_v2(&v2).is_ok());
}

#[test]
fn p4_member_mutation_uses_the_resolved_did() {
    let resolved = crate::groups::GroupMemberResolution {
        did: crate::ids::Did::parse("did:example:bob").unwrap(),
        handle: Some(crate::ids::Handle::parse("bob.example.com", "").unwrap()),
    };
    let request = crate::groups::GroupMemberMutationRequest {
        group: crate::ids::GroupRef::parse("did:example:group").unwrap(),
        member: crate::groups::GroupMemberRef::parse("bob.example.com", "").unwrap(),
        role: None,
        reason_text: None,
        leave_request_id: None,
        security: crate::groups::GroupSecurityRequirement::Required,
    };

    let v2 = super::resolved_group_member_request(request, &resolved);
    assert_eq!(
        v2.security,
        crate::groups::GroupSecurityRequirement::Required
    );
    assert_eq!(v2.member.as_str(), "did:example:bob");
}

#[test]
fn v2_cold_cache_route_uses_authoritative_policy_even_for_default_call() {
    let group = "did:example:group";
    let owner = authoritative_group(group, "group-e2ee", "owner", "active");
    assert_eq!(
        super::v2_member_mutation_route(group, &owner, false).unwrap(),
        super::V2MemberMutationRoute::OwnerP6,
        "an E2EE group must not become P4-only merely because the caller used the default hint"
    );

    let transport = authoritative_group(group, "transport-protected", "admin", "active");
    assert_eq!(
        super::v2_member_mutation_route(group, &transport, false).unwrap(),
        super::V2MemberMutationRoute::BaseOnly
    );
    assert!(super::v2_member_mutation_route(group, &transport, true).is_err());

    let unknown = crate::groups::GroupReadResult::from_raw_response(
        json!({
            "group": {
                "group_did": group,
                "my_role": "owner",
                "membership_status": "active",
                "group_policy": {
                    "message_security_profile": "future-secure-profile"
                }
            }
        }),
        Vec::new(),
    );
    assert!(matches!(
        super::v2_member_mutation_route(group, &unknown, false),
        Err(crate::ImError::LocalStateUnavailable { .. })
    ));

    let conflicting = crate::groups::GroupReadResult::from_raw_response(
        json!({
            "group": {
                "group_did": group,
                "my_role": "owner",
                "membership_status": "active",
                "group_policy": {
                    "message_security_profile": "group-e2ee"
                }
            },
            "group_snapshot": {
                "group_did": group,
                "group_policy": {
                    "message_security_profile": "transport-protected"
                }
            }
        }),
        Vec::new(),
    );
    assert!(matches!(
        super::v2_member_mutation_route(group, &conflicting, false),
        Err(crate::ImError::LocalStateUnavailable { .. })
    ));

    let malformed = crate::groups::GroupReadResult::from_raw_response(
        json!({
            "group": {
                "group_did": group,
                "my_role": "owner",
                "membership_status": "active",
                "group_policy": {
                    "message_security_profile": 42
                }
            }
        }),
        Vec::new(),
    );
    assert!(matches!(
        super::v2_member_mutation_route(group, &malformed, false),
        Err(crate::ImError::LocalStateUnavailable { .. })
    ));
}

#[test]
fn v2_p4_policy_is_exact_and_overrides_domain_local_projection() {
    let group = "did:example:group";
    let result_with_policy = |policy: serde_json::Value| {
        crate::groups::GroupReadResult::from_raw_response(
            json!({
                "group_did": group,
                "group_policy": policy,
                "group_snapshot": {
                    "group_did": group,
                    "my_role": "owner",
                    "membership_status": "active",
                    "required_security_profile": "transport-protected",
                    "group_policy": {
                        "message_security_profile": "transport-protected"
                    }
                }
            }),
            Vec::new(),
        )
    };

    let e2ee = result_with_policy(json!({"message_security_profile": "group-e2ee"}));
    assert!(super::authoritative_group_e2ee_classification(group, &e2ee).unwrap());

    let transport = result_with_policy(json!({
        "message_security_profile": "transport-protected"
    }));
    assert!(!super::authoritative_group_e2ee_classification(group, &transport).unwrap());

    for invalid in [
        json!({"message_security_profile": "transport"}),
        json!({"message_security_profile": "Transport-Protected"}),
        json!({"message_security_profile": " transport-protected"}),
        json!({"message_security_profile": "GROUP-E2EE"}),
        json!({"message_security_profile": "future-secure-profile"}),
        json!({}),
        json!({"message_security_profile": 42}),
    ] {
        let result = result_with_policy(invalid);
        assert!(matches!(
            super::authoritative_group_e2ee_classification(group, &result),
            Err(crate::ImError::LocalStateUnavailable { .. })
        ));
    }
}

#[test]
fn v2_admin_is_not_misclassified_as_an_illegal_p4_actor_or_p6_owner() {
    let group = "did:example:group";
    let admin = authoritative_group(group, "group-e2ee", "admin", "active");
    let error = super::v2_member_mutation_route(group, &admin, false)
        .expect_err("without a durable owner job the combined operation must fail before P4");
    assert!(matches!(
        error,
        crate::ImError::LocalStateUnavailable { .. }
    ));
}

#[test]
fn v2_leave_allows_p4_first_for_e2ee_group_and_rejects_mismatched_policy() {
    let group = "did:example:group";
    let member = authoritative_group(group, "group-e2ee", "member", "active");
    assert!(super::require_v2_leave_safe(group, &member, false).is_ok());
    assert!(super::require_v2_leave_safe(group, &member, true).is_ok());

    let transport = authoritative_group(group, "transport-protected", "member", "active");
    assert!(super::require_v2_leave_safe(group, &transport, false).is_ok());
    assert!(matches!(
        super::require_v2_leave_safe(group, &transport, true),
        Err(crate::ImError::InvalidInput { .. })
    ));

    let owner = authoritative_group(group, "group-e2ee", "owner", "active");
    assert!(matches!(
        super::require_v2_leave_safe(group, &owner, true),
        Err(crate::ImError::InvalidInput { .. })
    ));
}

#[test]
fn v2_idempotent_membership_retries_match_only_structured_service_codes() {
    let already = crate::ImError::Service {
        status_code: Some(409),
        code: Some("group.already_member".to_owned()),
        message: "localized".to_owned(),
        data: None,
    };
    let not_member = crate::ImError::Service {
        status_code: Some(404),
        code: Some("group.not_member".to_owned()),
        message: "localized".to_owned(),
        data: None,
    };
    assert!(super::group_error_is_already_member(&already));
    assert!(super::group_error_is_not_member(&not_member));
    assert!(!super::group_error_is_not_member(&already));
    assert!(!super::group_error_is_already_member(
        &crate::ImError::TransportUnavailable {
            detail: "server said already member in an untrusted message".to_owned(),
        }
    ));
}

#[test]
fn v2_authoritative_roster_matches_a_resolved_handle_by_did() {
    let did = crate::ids::Did::parse("did:example:bob").unwrap();
    let requested = crate::groups::GroupMemberResolution {
        did: did.clone(),
        handle: Some(crate::ids::Handle::parse("bob", "example.test").unwrap()),
    };
    let member = crate::groups::GroupMember {
        membership_id: None,
        peer_persona_id: None,
        did: Some(did),
        credential_did: None,
        handle: None,
        handle_binding_generation: None,
        role: Some("member".to_owned()),
        status: Some("active".to_owned()),
        joined_at: None,
        subject_type: Some("human".to_owned()),
    };

    assert!(super::active_member_matches_resolution(&member, &requested));
}

fn authoritative_group(
    group_did: &str,
    security_profile: &str,
    role: &str,
    status: &str,
) -> crate::groups::GroupReadResult {
    crate::groups::GroupReadResult::from_raw_response(
        json!({
            "group": {
                "group_did": group_did,
                "my_role": role,
                "membership_status": status,
                "group_policy": {
                    "message_security_profile": security_profile
                }
            }
        }),
        Vec::new(),
    )
}

fn test_did_bundle() -> anp::authentication::DidDocumentBundle {
    anp::authentication::create_did_wba_document(
        "example.test",
        anp::authentication::DidDocumentOptions {
            path_segments: vec!["alice".to_owned()],
            domain: Some("example.test".to_owned()),
            challenge: Some("group-v2-only-test".to_owned()),
            ..anp::authentication::DidDocumentOptions::default()
        },
    )
    .unwrap()
}

fn unique_temp_root() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "im-core-group-v2-only-{}-{nanos}",
        std::process::id()
    ))
}
