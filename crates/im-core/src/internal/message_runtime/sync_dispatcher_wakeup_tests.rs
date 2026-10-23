use super::DispatchState;

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
