use super::*;

fn scope() -> Scope {
    Scope {
        owner: "owner".into(),
        did: "did:example:owner".into(),
        device: "device".into(),
        generation: 1,
        signing_key: "did:example:owner#sign-device".into(),
        e2ee_key: "did:example:owner#e2ee-device".into(),
        group: "did:example:group".into(),
    }
}

#[test]
fn group_roster_proof_is_scoped_and_version_bound() {
    let scope = scope();
    let members = BTreeSet::from(["did:example:owner".to_owned()]);
    let mut cache = GroupSendState::default();
    assert!(!cache.contains(&scope, "1", &members));
    cache.confirm(scope.clone(), "1".into(), members.clone());
    assert!(cache.contains(&scope, "1", &members));
    assert!(!cache.contains(&scope, "2", &members));
    assert!(!cache.contains(&scope, "1", &BTreeSet::new()));
    for kind in 0..7 {
        let mut changed = scope.clone();
        match kind {
            0 => changed.owner = "other".into(),
            1 => changed.did = "did:example:other".into(),
            2 => changed.device = "sibling".into(),
            3 => changed.generation = 2,
            4 => changed.group = "did:example:other-group".into(),
            5 => changed.signing_key = "did:example:owner#new-signing".into(),
            _ => changed.e2ee_key = "did:example:owner#new-e2ee".into(),
        }
        assert!(!cache.contains(&changed, "1", &members));
    }
    assert!(!GroupSendState::default().contains(&scope, "1", &members));
}

#[test]
fn group_roster_proof_storage_is_bounded() {
    let mut cache = GroupSendState::default();
    for index in 0..129 {
        let mut key = scope();
        key.group = format!("group-{index}");
        cache.confirm(key, "1".into(), BTreeSet::new());
    }
    assert!(cache.entries.len() <= 128);
}
