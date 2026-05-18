use super::host_notify_sink::HostNotifySinkImpl;
use std::sync::Arc;

pub fn host_notify_for_bridge_created_session(
    supervisor_host_notify: &Arc<HostNotifySinkImpl>,
) -> Arc<HostNotifySinkImpl> {
    Arc::clone(supervisor_host_notify)
}
