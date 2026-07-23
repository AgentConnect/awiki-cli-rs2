pub struct SystemNotificationService<'a> {
    client: &'a crate::core::ImClient,
}

impl<'a> SystemNotificationService<'a> {
    pub(crate) fn new(client: &'a crate::core::ImClient) -> Self {
        Self { client }
    }

    pub async fn list(
        &self,
        query: super::SystemNotificationListQuery,
    ) -> crate::ImResult<Vec<super::SystemNotificationSnapshot>> {
        #[cfg(feature = "sqlite")]
        {
            let limit = query.limit.unwrap_or(100).clamp(1, 500);
            let protocol_device_id = exact_device_id(self.client)?;
            let db = self.client.core_inner().local_state_db().await?;
            db.list_system_notifications(
                self.client.current_identity().id.as_str(),
                self.client.did().as_str(),
                protocol_device_id,
                query.include_terminal,
                limit,
            )
            .await
        }
        #[cfg(not(feature = "sqlite"))]
        {
            let _ = query;
            Err(crate::ImError::unsupported(
                "system-notification-local-state",
            ))
        }
    }

    pub async fn get(
        &self,
        event_id: &str,
    ) -> crate::ImResult<Option<super::SystemNotificationSnapshot>> {
        let event_id = event_id.trim();
        if event_id.is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("event_id".to_owned()),
                "event_id is required",
            ));
        }
        #[cfg(feature = "sqlite")]
        {
            let protocol_device_id = exact_device_id(self.client)?;
            let db = self.client.core_inner().local_state_db().await?;
            db.get_system_notification(
                self.client.current_identity().id.as_str(),
                self.client.did().as_str(),
                protocol_device_id,
                event_id,
            )
            .await
        }
        #[cfg(not(feature = "sqlite"))]
        {
            Err(crate::ImError::unsupported(
                "system-notification-local-state",
            ))
        }
    }

    pub async fn watch(
        &self,
        query: super::SystemNotificationListQuery,
    ) -> crate::ImResult<super::SystemNotificationChangeSession> {
        let items = self.list(query).await?;
        Ok(self
            .client
            .system_notification_store()
            .watch_for_client(self.client, items)?)
    }
}

fn exact_device_id(client: &crate::core::ImClient) -> crate::ImResult<String> {
    client.exact_protocol_device_id().map_err(|_| {
        crate::ImError::invalid_input(
            Some("identity.protocol_device_id".to_owned()),
            "system notifications require an exact-device identity",
        )
    })
}
