use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde_json::{json, Value};

use super::*;

const DID: &str = "did:wba:example.test:users:community:e1_fixture";
const ACCOUNT_ID: &str = "community-fixture-account";
const KEY_ID: &str = "did:wba:example.test:users:community:e1_fixture#device-primary-sign";

fn template_claims() -> Value {
    let fixture = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/testdata/message-sync/community-device-access-v1.json"
    ));
    let fixture: Value = serde_json::from_str(
        &fixture
            .replace("$DID", DID)
            .replace("$ACCOUNT_ID", ACCOUNT_ID),
    )
    .unwrap();
    fixture["claims"].clone()
}

fn expected_device() -> ExpectedDeviceAccess<'static> {
    ExpectedDeviceAccess {
        did: DID,
        user_id: ACCOUNT_ID,
        device_id: "device-primary",
        key_id: KEY_ID,
        auth_generation: 1,
        role: crate::internal::identity_device_state::DeviceAuthorizationRole::Admin,
        management_ready: true,
    }
}

fn wire_token(mut claims: Value) -> String {
    let now = OffsetDateTime::now_utc().unix_timestamp();
    claims["iat"] = json!(now);
    claims["nbf"] = json!(now);
    claims["exp"] = json!(now + 300);
    claims["jti"] = json!("community-unit-fixture");
    // Core checks claims from the authenticated Home transport. Cryptographic
    // token verification remains covered by the server's owning tests.
    format!(
        "e30.{}.fixture-signature",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
    )
}

#[test]
fn community_device_access_fixture_uses_existing_exact_device_contract() {
    let claims = template_claims();
    let expected = expected_device();
    validate_device_access_token(&wire_token(claims.clone()), &expected).unwrap();
    for (field, replacement) in [
        ("device_id", json!("second-device")),
        ("auth_generation", json!(2)),
        ("user_id", json!(DID)),
        ("key_id", json!(format!("{DID}#key-1"))),
        ("scopes", json!(["message:connect"])),
    ] {
        let mut changed = claims.clone();
        changed[field] = replacement;
        assert!(
            validate_device_access_token(&wire_token(changed), &expected).is_err(),
            "{field} must remain fenced"
        );
    }
}
