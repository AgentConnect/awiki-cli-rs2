use super::*;

#[test]
fn group_policy_selects_encryption_without_downgrading_explicit_intent() {
    use MessageSecurityMode::*;
    assert_eq!(group_mode(DefaultPlain, true).unwrap(), GroupE2ee);
    assert_eq!(group_mode(DefaultPlain, false).unwrap(), Plain);
    assert_eq!(group_mode(E2eeRequired, true).unwrap(), GroupE2ee);
    assert!(group_mode(Plain, true).is_err());
    assert!(group_mode(E2eeRequired, false).is_err());
    assert!(group_mode(GroupE2ee, false).is_err());
    assert!(group_mode(SecureDirect, true).is_err());
    assert!(group_mode(SecureDirect, false).is_err());
}
