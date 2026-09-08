use super::*;
#[test]
fn generic_capability_commands_resolve_with_local_identity_and_sdk_ownership() {
    use crate::command_catalog::{CommandOwner, CutoverStatus};
    for (parent, operation, owner) in [
        ("proof", "sign-object", CommandOwner::ImCoreIdentity),
        ("http", "request", CommandOwner::ImCoreAuth),
    ] {
        let parsed = parse_args(
            ["--format", "json", "--identity", "alice", parent, operation]
                .into_iter()
                .map(str::to_owned),
        )
        .unwrap();
        assert_eq!(parsed.name, format!("{parent}.{operation}"));
        assert_eq!(parsed.globals.identity, "alice");
        assert!(parsed.args.is_empty());
        assert!(parsed.flags.is_empty());
        assert_eq!(
            crate::command_catalog::cutover_status(&parsed.name),
            CutoverStatus::ImCore
        );
        assert_eq!(crate::command_catalog::primary_owner(&parsed.name), owner);
        assert!(
            crate::command_catalog::lookup(&parsed.name)
                .unwrap()
                .side_effect
        );
    }
}

#[test]
fn old_product_commands_are_removed_and_sender_assertion_is_generic() {
    assert!(crate::command_catalog::lookup("node-publication.sign").is_none());
    assert!(parse_args(
        ["node-publication", "--help"]
            .into_iter()
            .map(str::to_owned)
    )
    .is_err());
    let parsed = parse_args(
        [
            "msg",
            "send",
            "--to",
            "bob.example.org",
            "--payload",
            "{}",
            "--expected-sender-did",
            "did:wba:example.org:alice",
        ]
        .into_iter()
        .map(str::to_owned),
    )
    .unwrap();
    assert_eq!(parsed.name, "msg.send");
    assert!(parsed.flags.contains_key("expected-sender-did"));
}
