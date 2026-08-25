use anp::authentication::{
    parse_did_wba_e1, resolve_current_did, verify_active_e1_document, verify_transition_hop,
    ANP_DID_SUPERSEDED, ANP_DID_TRANSITION_CONFLICT, ANP_DID_TRANSITION_INVALID,
    DEFAULT_MAX_TRANSITION_HOPS,
};

#[test]
fn frozen_transition_api_is_available() {
    assert_eq!(ANP_DID_SUPERSEDED, 1019);
    assert_eq!(ANP_DID_TRANSITION_INVALID, 1020);
    assert_eq!(ANP_DID_TRANSITION_CONFLICT, 1021);
    assert_eq!(DEFAULT_MAX_TRANSITION_HOPS, 8);
    let _parse = parse_did_wba_e1;
    let _active = verify_active_e1_document;
    let _hop = verify_transition_hop;
    let _resolve = resolve_current_did;
}
