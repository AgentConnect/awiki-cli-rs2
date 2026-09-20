use super::*;
#[test]
fn http_request_has_sdk_auth_ownership_and_explicit_side_effect() {
    let parsed = parse_args(
        ["--format", "json", "--identity", "alice", "http", "request"]
            .into_iter()
            .map(str::to_owned),
    )
    .unwrap();
    assert_eq!(parsed.name, "http.request");
    assert_eq!(parsed.globals.identity, "alice");
    assert_eq!(
        command_catalog::cutover_status(&parsed.name),
        command_catalog::CutoverStatus::ImCore
    );
    assert_eq!(
        command_catalog::primary_owner(&parsed.name),
        command_catalog::CommandOwner::ImCoreAuth
    );
    assert!(command_catalog::lookup(&parsed.name).unwrap().side_effect);
}
