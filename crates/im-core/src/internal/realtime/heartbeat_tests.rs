use std::time::Duration;

use super::{SESSION_PING_INTERVAL, SESSION_PING_TIMEOUT};

#[test]
fn heartbeat_detects_half_open_session_within_thirty_five_seconds() {
    assert_eq!(SESSION_PING_INTERVAL, Duration::from_secs(20));
    assert_eq!(SESSION_PING_TIMEOUT, Duration::from_secs(15));
    assert_eq!(
        SESSION_PING_INTERVAL + SESSION_PING_TIMEOUT,
        Duration::from_secs(35)
    );
}
