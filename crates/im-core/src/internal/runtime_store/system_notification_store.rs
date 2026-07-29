use std::sync::Arc;

use tokio::sync::broadcast;

const CHANGE_BUFFER: usize = 128;

#[derive(Debug)]
pub(crate) struct SystemNotificationStore {
    owner_identity_id: String,
    owner_did: String,
    protocol_device_id: Option<String>,
    sender: broadcast::Sender<crate::system_notifications::SystemNotificationChange>,
}

impl SystemNotificationStore {
    pub(crate) fn new_for_client(client: &crate::core::ImClient) -> Arc<Self> {
        let (sender, _) = broadcast::channel(CHANGE_BUFFER);
        Arc::new(Self {
            owner_identity_id: client.current_identity().id.as_str().to_owned(),
            owner_did: client.did().as_str().to_owned(),
            protocol_device_id: client.exact_protocol_device_id().ok(),
            sender,
        })
    }

    pub(crate) fn watch_for_client(
        self: &Arc<Self>,
        client: &crate::core::ImClient,
        items: Vec<crate::system_notifications::SystemNotificationSnapshot>,
    ) -> crate::ImResult<crate::system_notifications::SystemNotificationChangeSession> {
        self.ensure_owner(client)?;
        Ok(
            crate::system_notifications::SystemNotificationChangeSession::new(
                self.clone(),
                self.sender.subscribe(),
                vec![crate::system_notifications::SystemNotificationChange::Reset { items }],
            ),
        )
    }

    pub(crate) fn emit_committed(
        &self,
        client: &crate::core::ImClient,
        item: crate::system_notifications::SystemNotificationSnapshot,
    ) {
        if self.ensure_owner(client).is_err() {
            return;
        }
        let _ = self
            .sender
            .send(crate::system_notifications::SystemNotificationChange::Changed { item });
    }

    pub(crate) fn repair_required(
        &self,
        reason: &str,
    ) -> crate::system_notifications::SystemNotificationChange {
        crate::system_notifications::SystemNotificationChange::RepairRequired {
            reason: reason.to_owned(),
        }
    }

    fn ensure_owner(&self, client: &crate::core::ImClient) -> crate::ImResult<()> {
        if client.current_identity().id.as_str() != self.owner_identity_id
            || client.did().as_str() != self.owner_did
            || client.exact_protocol_device_id().ok().as_deref()
                != self.protocol_device_id.as_deref()
        {
            return Err(crate::ImError::invalid_input(
                Some("client".to_owned()),
                "system notification store owner does not match client identity",
            ));
        }
        Ok(())
    }
}
