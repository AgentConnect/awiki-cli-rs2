use std::sync::{Arc, Mutex};

use crate::dto::{
    error::DartImError,
    realtime::{
        DartRealtimeCapability, DartRealtimeEvent, DartRealtimeOptions, DartRealtimeStatus,
    },
};
use crate::frb_generated::StreamSink;

pub struct DartRealtimeSession {
    session: Mutex<Option<im_core::realtime::RealtimeSession>>,
    event_stream_attached: Mutex<bool>,
}

impl DartRealtimeSession {
    fn new(session: im_core::realtime::RealtimeSession) -> Self {
        Self {
            session: Mutex::new(Some(session)),
            event_stream_attached: Mutex::new(false),
        }
    }

    fn take_event_receiver(&self) -> Result<im_core::realtime::RealtimeEventStream, DartImError> {
        let mut attached = self
            .event_stream_attached
            .lock()
            .map_err(|_| DartImError::internal("realtime session lock poisoned"))?;
        if *attached {
            return Err(DartImError::invalid_input(
                Some("session".to_string()),
                "realtime event stream is already attached",
            ));
        }
        let mut guard = self
            .session
            .lock()
            .map_err(|_| DartImError::internal("realtime session lock poisoned"))?;
        let session = guard
            .as_mut()
            .ok_or_else(|| DartImError::object_closed("DartRealtimeSession"))?;
        let receiver = session.subscribe().map_err(DartImError::from)?;
        *attached = true;
        Ok(receiver)
    }

    fn status(&self) -> Result<DartRealtimeStatus, DartImError> {
        let guard = self
            .session
            .lock()
            .map_err(|_| DartImError::internal("realtime session lock poisoned"))?;
        let session = guard
            .as_ref()
            .ok_or_else(|| DartImError::object_closed("DartRealtimeSession"))?;
        Ok(session.status().into())
    }

    async fn stop(&self) -> Result<(), DartImError> {
        let session = self
            .session
            .lock()
            .map_err(|_| DartImError::internal("realtime session lock poisoned"))?
            .take();
        if let Some(session) = session {
            session.stop().await.map_err(DartImError::from)?;
        }
        Ok(())
    }
}

impl Drop for DartRealtimeSession {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.session.lock() {
            if let Some(session) = guard.take() {
                drop(session);
            }
        }
    }
}

pub fn realtime_capability(
    _client: &Arc<crate::api::client::DartImClient>,
) -> Result<DartRealtimeCapability, DartImError> {
    Ok(DartRealtimeCapability {
        status_supported: true,
        connect_supported: true,
        runner_exposed: true,
        reason: None,
    })
}

pub async fn realtime_status(
    client: &Arc<crate::api::client::DartImClient>,
) -> Result<DartRealtimeStatus, DartImError> {
    let inner = client.clone_inner()?;
    inner
        .realtime()
        .status()
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn realtime_connect(
    client: &Arc<crate::api::client::DartImClient>,
) -> Result<(), DartImError> {
    let session = realtime_start(
        client,
        DartRealtimeOptions {
            reconnect: "disabled".to_string(),
            event_buffer: 128,
            reconnect_delay_ms: None,
            reconnect_base_delay_ms: None,
            reconnect_max_delay_ms: None,
            reconnect_max_attempts: None,
            subscriptions: vec![
                "messages".to_string(),
                "groups".to_string(),
                "notifications".to_string(),
            ],
        },
    )
    .await?;
    realtime_stop(&session).await
}

pub async fn realtime_start(
    client: &Arc<crate::api::client::DartImClient>,
    options: DartRealtimeOptions,
) -> Result<Arc<DartRealtimeSession>, DartImError> {
    let options = options.try_into()?;
    let inner = client.clone_inner()?;
    let session = inner
        .realtime()
        .start_async(options)
        .await
        .map_err(DartImError::from)?;
    Ok(Arc::new(DartRealtimeSession::new(session)))
}

pub async fn realtime_stop(session: &Arc<DartRealtimeSession>) -> Result<(), DartImError> {
    session.stop().await
}

pub fn realtime_session_status(
    session: &Arc<DartRealtimeSession>,
) -> Result<DartRealtimeStatus, DartImError> {
    session.status()
}

pub async fn realtime_event_stream(
    session: &Arc<DartRealtimeSession>,
    sink: StreamSink<DartRealtimeEvent>,
) -> Result<(), DartImError> {
    let mut receiver = session.take_event_receiver()?;
    let session_for_worker = Arc::downgrade(session);
    tokio::spawn(async move {
        while let Some(event) = receiver.recv().await {
            let event = crate::mapping::from_core::realtime_event_to_dart(event);
            if sink.add(event).is_err() {
                if let Some(session) = session_for_worker.upgrade() {
                    let _ = session.stop().await;
                }
                break;
            }
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use im_core::{
        IdentityRegistryPaths, IdentitySelector, ImCore, ImCoreConfig, ImCorePaths,
        LocalStatePaths, MessageTransportPolicy, RuntimePaths, ServiceEndpoint,
    };

    use super::*;

    #[tokio::test]
    async fn dart_realtime_session_allows_only_one_event_receiver() {
        let client = Arc::new(crate::api::client::DartImClient::new(test_client()));
        let session = realtime_start(&client, test_options()).await.unwrap();

        let _receiver = session.take_event_receiver().unwrap();
        let error = session.take_event_receiver().unwrap_err();

        assert_eq!(error.code, "invalid_input");
        assert_eq!(error.field.as_deref(), Some("session"));
        assert!(error.message.contains("already attached"));
        realtime_stop(&session).await.unwrap();
    }

    #[tokio::test]
    async fn dart_realtime_stop_disposes_session_handle() {
        let client = Arc::new(crate::api::client::DartImClient::new(test_client()));
        let session = realtime_start(&client, test_options()).await.unwrap();

        realtime_stop(&session).await.unwrap();
        let error = realtime_session_status(&session).unwrap_err();

        assert_eq!(error.code, "object_closed");
        assert!(error.message.contains("DartRealtimeSession"));
        realtime_stop(&session).await.unwrap();
    }

    fn test_options() -> DartRealtimeOptions {
        DartRealtimeOptions {
            reconnect: "disabled".to_owned(),
            event_buffer: 4,
            reconnect_delay_ms: None,
            reconnect_base_delay_ms: None,
            reconnect_max_delay_ms: None,
            reconnect_max_attempts: None,
            subscriptions: vec!["messages".to_owned()],
        }
    }

    fn test_client() -> im_core::ImClient {
        let root = unique_temp_root();
        write_ready_identity(&root, "alice", "test-token");
        ImCore::new(
            ImCoreConfig {
                service_base_url: ServiceEndpoint::parse("http://127.0.0.1:9").unwrap(),
                did_domain: "awiki.test".to_owned(),
                user_service_endpoint: None,
                message_service_endpoint: None,
                mail_service_endpoint: None,
                anp_service_endpoint: None,
                anp_service_did: None,
                ca_bundle: None,
                transport_policy: MessageTransportPolicy::Auto,
            },
            ImCorePaths {
                identities: IdentityRegistryPaths {
                    identity_root_dir: root.join("identities"),
                    registry_path: root.join("identities").join("registry.json"),
                    default_identity_path: Some(root.join("identities").join("default")),
                },
                local_state: LocalStatePaths {
                    sqlite_path: root.join("local").join("im.sqlite"),
                },
                runtime: RuntimePaths {
                    cache_dir: root.join("cache"),
                    temp_dir: root.join("tmp"),
                },
            },
        )
        .unwrap()
        .client(IdentitySelector::LocalAlias("alice".to_owned()))
        .unwrap()
    }

    fn write_ready_identity(root: &std::path::Path, alias: &str, token: &str) {
        let identities = root.join("identities");
        std::fs::create_dir_all(&identities).unwrap();
        std::fs::write(identities.join("default"), format!("{alias}\n")).unwrap();
        std::fs::write(
            identities.join("registry.json"),
            format!(
                r#"{{
                  "default_identity": "{alias}",
                  "identities": [{{
                    "id": "{alias}-id",
                    "did": "did:example:{alias}",
                    "handle": "{alias}.awiki.test",
                    "display_name": "{alias}",
                    "local_alias": "{alias}",
                    "is_default": true,
                    "ready_for_auth": true,
                    "ready_for_messaging": true,
                    "missing": []
                  }}]
                }}"#
            ),
        )
        .unwrap();
        let dir = identities.join(alias);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("did.json"),
            format!(r#"{{"id":"did:example:{alias}"}}"#),
        )
        .unwrap();
        std::fs::write(dir.join("private.key"), "test-private-key").unwrap();
        std::fs::write(
            dir.join("auth.json"),
            format!(r#"{{"jwt_token":"{token}"}}"#),
        )
        .unwrap();
    }

    fn unique_temp_root() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "im-core-dart-realtime-{}-{nanos}",
            std::process::id()
        ))
    }
}
