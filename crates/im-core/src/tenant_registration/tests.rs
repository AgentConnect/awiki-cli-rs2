use std::collections::BTreeMap;

use serde_json::json;

use super::*;
use crate::site::SiteDomain;

#[test]
fn tenant_registration_wire_error_keeps_public_code_through_json_rpc_decoder() {
    let error = crate::internal::json_rpc::decode_response(
        &serde_json::to_vec(&json!({"jsonrpc":"2.0","id":"req-1","error":{
            "code":-32003,"message":"Denied","data":{
                "awiki_code":"tenant_registration.forbidden","retryable":false
            }
        }}))
        .unwrap(),
    )
    .unwrap_err();
    let crate::ImError::Service { code, data, .. } = error else {
        panic!("service error required");
    };
    assert_eq!(code.as_deref(), Some("tenant_registration.forbidden"));
    assert_eq!(data.unwrap()["json_rpc_code"], -32003);
}

fn domain() -> SiteDomain {
    SiteDomain::parse("tenant.example.org").unwrap()
}

fn change() -> ConfigureRegistration {
    ConfigureRegistration {
        domain: domain(),
        operation_id: "b15c3ef6-beb6-4c2d-a6a3-079873e026fd".into(),
        expected_revision: 1,
        mode: Some(RegistrationMode::Allowlist),
        allowed_contact_types: Some(vec![ContactType::Phone, ContactType::Email]),
        add_entries: vec![],
        remove_entry_ids: vec![],
    }
}

#[test]
fn tenant_registration_contacts_normalize_without_merging_addresses_or_leaking_debug_values() {
    let mut contact = AllowlistContact {
        contact_type: ContactType::Email,
        value: " Alice+tag@Example.ORG ".into(),
    };
    contact.normalize().unwrap();
    assert_eq!(contact.value, "alice+tag@example.org");
    assert!(!format!("{contact:?}").contains("alice"));
    for value in ["2025550123", "+00000000", "+1202555abc"] {
        assert!(AllowlistContact {
            contact_type: ContactType::Phone,
            value: value.into()
        }
        .normalize()
        .is_err());
    }
}

#[test]
fn tenant_registration_changes_are_bounded_and_semantically_stable() {
    let mut first = change();
    first.add_entries = vec![AllowlistContact {
        contact_type: ContactType::Email,
        value: "A@example.org".into(),
    }];
    first.normalize().unwrap();
    let mut second = first.clone();
    second.allowed_contact_types = Some(vec![ContactType::Phone, ContactType::Email]);
    second.normalize().unwrap();
    assert_eq!(first, second);
    second.add_entries.push(AllowlistContact {
        contact_type: ContactType::Email,
        value: " a@EXAMPLE.org ".into(),
    });
    assert!(second.normalize().is_err());
    let mut invalid = change();
    invalid.expected_revision = 0;
    assert!(invalid.normalize().is_err());
    invalid = change();
    invalid.operation_id = "00000000-0000-0000-0000-000000000000".into();
    assert!(invalid.normalize().is_err());
    assert!(serde_json::from_value::<ConfigureRegistration>(
        json!({"domain":"tenant.example.org","admin":true})
    )
    .is_err());
}

#[test]
fn tenant_registration_policy_rejects_cross_domain_or_unversioned_tenant_results() {
    let mut policy = RegistrationPolicy {
        contract: "tenant-registration-policy-v1".into(),
        domain: domain(),
        source: RegistrationPolicySource::Tenant,
        mode: RegistrationMode::Open,
        revision: Some(1),
        allowed_contact_types: vec![ContactType::Phone, ContactType::Email],
        effective_methods: vec![ContactType::Phone],
        entry_counts: None,
    };
    policy.validate(&domain()).unwrap();
    assert!(policy
        .validate(&SiteDomain::parse("another.example.org").unwrap())
        .is_err());
    policy.revision = None;
    assert!(policy.validate(&domain()).is_err());
    policy.source = RegistrationPolicySource::Platform;
    policy.validate(&domain()).unwrap();
}

#[test]
fn tenant_registration_default_list_cannot_accidentally_expose_full_contacts() {
    let request = ListRegistrationEntries {
        domain: domain(),
        limit: 100,
        after: None,
        contact_type: None,
        include_values: false,
    };
    let mut page = AllowlistPage {
        domain: domain(),
        revision: 1,
        next_cursor: None,
        entries: vec![AllowlistEntry {
            entry_id: "11d91eb6-54bb-4ad5-957a-5b870e3d1c98".into(),
            contact_type: ContactType::Email,
            masked_value: "a***@example.org".into(),
            value: Some("allowed@example.org".into()),
        }],
    };
    assert!(page.validate(&request).is_err());
    assert!(!format!("{page:?}").contains("allowed@example.org"));
    page.entries[0].value = None;
    page.validate(&request).unwrap();
}

#[test]
fn tenant_registration_historical_receipts_must_match_the_requested_operation_and_domain() {
    let receipt = RegistrationOperation {
        domain: domain(),
        operation_id: "b15c3ef6-beb6-4c2d-a6a3-079873e026fd".into(),
        revision: 2,
        current_revision: 5,
        mode: RegistrationMode::Open,
        allowed_contact_types: vec![ContactType::Phone],
        entry_counts: BTreeMap::new(),
        added: 0,
        removed: 0,
        changed: true,
        replayed: true,
    };
    receipt.validate(&domain(), &receipt.operation_id).unwrap();
    assert!(receipt.validate(&domain(), "wrong-operation").is_err());
    assert!(receipt
        .validate(
            &SiteDomain::parse("other.example.org").unwrap(),
            &receipt.operation_id
        )
        .is_err());
}

struct NoSession;
impl crate::internal::auth::session::SessionProvider for NoSession {
    fn ensure_session(
        &self,
        scope: crate::auth::AuthScope,
    ) -> crate::ImResult<crate::auth::SessionBundle> {
        assert_eq!(scope, crate::auth::AuthScope::UserProfile);
        Err(crate::ImError::AuthRequired)
    }
    fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
        Err(crate::ImError::AuthRequired)
    }
    fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
        Err(crate::ImError::AuthRequired)
    }
}
struct NoNetwork;
impl crate::internal::transport::AuthenticatedRpcTransport for NoNetwork {
    fn authenticated_rpc(
        &mut self,
        _: &str,
        _: &str,
        _: serde_json::Value,
    ) -> crate::ImResult<serde_json::Value> {
        panic!("authentication must be checked before contacting the policy endpoint")
    }
}

#[test]
fn tenant_registration_management_requires_user_auth_before_transport() {
    let mut runtime = super::service::Runtime {
        session: NoSession,
        transport: NoNetwork,
    };
    assert_eq!(
        runtime.call("get_policy", json!({"domain":"tenant.example.org"})),
        Err(crate::ImError::AuthRequired)
    );
}
