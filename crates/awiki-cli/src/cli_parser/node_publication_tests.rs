use super::*;
#[test]
fn node_publication_commands_resolve_with_local_identity_and_sdk_ownership() {
    use crate::command_catalog::{CommandOwner, CutoverStatus};
    for (operation, owner) in [
        ("sign", CommandOwner::ImCoreIdentity),
        ("request", CommandOwner::ImCoreAuth),
        ("notify", CommandOwner::ImCoreMessages),
    ] {
        let parsed = parse_args(
            [
                "--format",
                "json",
                "--identity",
                "alice",
                "node-publication",
                operation,
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        assert_eq!(parsed.name, format!("node-publication.{operation}"));
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
