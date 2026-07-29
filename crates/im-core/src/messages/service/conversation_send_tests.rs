use crate::messages::MessageSecurityMode;

#[test]
fn secure_conversation_modes_use_existing_security_runtime() {
    for security in [
        MessageSecurityMode::E2eeRequired,
        MessageSecurityMode::SecureDirect,
        MessageSecurityMode::GroupE2ee,
    ] {
        assert!(super::conversation_send_uses_security_runtime(&security));
    }
}

#[test]
fn plain_conversation_modes_keep_durable_local_echo_runtime() {
    for security in [
        MessageSecurityMode::DefaultPlain,
        MessageSecurityMode::Plain,
    ] {
        assert!(!super::conversation_send_uses_security_runtime(&security));
    }
}
