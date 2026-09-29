use super::*;
use crate::internal::identity_wire::profile::{
    build_clear_avatar_rpc_call, build_set_avatar_rpc_call,
};

fn request() -> SetAvatarRequest {
    SetAvatarRequest {
        request_id: "1e15bcb3-97f6-4bc0-a4e7-16eeb70a18fa".to_owned(),
        expected_profile_version: "7".to_owned(),
        image_jpeg: vec![0xff, 0xd8, 0xff, 0xd9],
    }
}

#[test]
fn avatar_wire_reuses_exact_operation_and_rejects_invalid_inputs() {
    let request = request();
    let call = build_set_avatar_rpc_call(request.clone()).unwrap();
    let retry = build_set_avatar_rpc_call(request.clone()).unwrap();
    assert_eq!(call.method, "set_avatar");
    assert_eq!(call.params, retry.params);
    assert_eq!(call.params.as_object().unwrap().len(), 3);
    assert!(call.params.get("did").is_none());
    let clear = build_clear_avatar_rpc_call(ClearAvatarRequest {
        request_id: request.request_id.clone(),
        expected_profile_version: "8".to_owned(),
    })
    .unwrap();
    assert_eq!(clear.method, "clear_avatar");
    assert_eq!(clear.params.as_object().unwrap().len(), 2);
    let mut bad = request.clone();
    bad.image_jpeg = vec![0; 512 * 1024 + 1];
    assert!(build_set_avatar_rpc_call(bad).is_err());
    for version in [
        "",
        "-1",
        "01",
        "1.0",
        "9223372036854775808",
        "99999999999999999999",
    ] {
        let mut bad = request.clone();
        bad.expected_profile_version = version.to_owned();
        assert!(build_set_avatar_rpc_call(bad).is_err());
    }
    let mut bad = request;
    bad.request_id = "not-a-uuid".to_owned();
    assert!(build_set_avatar_rpc_call(bad).is_err());
}

#[test]
fn avatar_clear_stays_explicit_on_wire_and_images_are_redacted_from_debug() {
    let mut profile =
        crate::identity::Profile::new(crate::ids::Did::parse("did:example:me").unwrap());
    assert!(profile.to_wire_profile_value().get("avatar_uri").is_none());
    profile.avatar_uri_present = true;
    let wire = profile.to_wire_profile_value();
    assert!(wire["avatar_uri"].is_null() && wire["avatar_url"].is_null());
    let request = request();
    let debug = format!("{request:?}");
    assert!(!debug.contains("255") && !debug.contains("image_jpeg"));
    assert!(debug.contains("image_bytes: 4"));
}
