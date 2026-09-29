use super::{avatar_members_from_value, group_summary_from_value};
use serde_json::json;

#[test]
fn avatar_summary_preserves_host_order_and_rejects_partial_or_unversioned_input() {
    let members = json!([
        {"member_key":"z","member_did":"did:example:human"},
        {"member_key":"a","member_did":"did:example:agent","member_handle":"agent.example"}
    ]);
    let value =
        json!({"group_did":"did:example:group","group_state_version":"8","avatar_members":members});
    let summary = group_summary_from_value(value.clone()).unwrap();
    let tiles = summary.avatar_members.unwrap();
    assert_eq!(tiles[0].member_key, "z");
    assert_eq!(tiles[1].member_key, "a");
    assert_eq!(summary.group_state_version.as_deref(), Some("8"));
    assert!(avatar_members_from_value(&json!({"avatar_members":members})).is_none());
    assert!(avatar_members_from_value(
        &json!({"group_state_version":"08","avatar_members":members})
    )
    .is_none());
    for bad in [
        json!([members[0], members[0]]),
        json!([members[0], {"member_key":"bad","member_did":"invalid"}]),
        json!([members[0], members[1], members[0], members[1], members[0]]),
    ] {
        assert!(avatar_members_from_value(
            &json!({"group_state_version":"8","avatar_members":bad})
        )
        .is_none());
    }
    assert_eq!(
        avatar_members_from_value(&json!({"group_state_version":"8","avatar_members":[]})),
        Some(vec![])
    );
    assert!(
        group_summary_from_value(json!({"group_did":"did:example:group"}))
            .unwrap()
            .avatar_members
            .is_none()
    );
}
