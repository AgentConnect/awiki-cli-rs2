use super::DispatchState;

#[test]
#[cfg(feature = "group-e2ee")]
fn a_group_hint_during_admission_keeps_the_follow_up_wakeup() {
    let mut state = DispatchState::default();
    state.mark_group_repair_due("owner-a".into());
    let observed = state.group_repairs_due.clone();
    state.mark_group_repair_due("owner-a".into());
    state.mark_group_repair_due("owner-b".into());
    state.consume_group_repair_wake("owner-a", observed.get("owner-a"));
    assert_eq!(state.group_repairs_due.len(), 2);
    let generation = state.group_repairs_due["owner-a"];
    state.consume_group_repair_wake("owner-a", Some(&generation));
    assert!(!state.group_repairs_due.contains_key("owner-a"));
    assert!(state.group_repairs_due.contains_key("owner-b"));
}

#[test]
fn a_reply_arriving_during_maintenance_admission_keeps_its_wakeup() {
    let mut state = DispatchState::default();
    state.mark_p5_reply_due("owner-a".into());
    let observed = state.p5_replies_due.clone();
    state.mark_p5_reply_due("owner-a".into());
    state.mark_p5_reply_due("owner-b".into());
    state.consume_p5_reply_wake("owner-a", observed.get("owner-a"));
    assert!(state.p5_replies_due.contains_key("owner-a"));
    let generation = state.p5_replies_due["owner-a"];
    state.consume_p5_reply_wake("owner-a", Some(&generation));
    assert!(!state.p5_replies_due.contains_key("owner-a"));
    assert!(state.p5_replies_due.contains_key("owner-b"));
}
