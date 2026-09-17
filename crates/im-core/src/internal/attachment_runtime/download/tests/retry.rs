use super::*;
use crate::internal::transport::AsyncAttachmentObjectResponse;

struct FaultTransport {
    inner: RecordingTransport,
    failures: VecDeque<(&'static str, crate::ImError)>,
    phases: Rc<RefCell<Vec<&'static str>>>,
    stall_discovery: Option<std::sync::Arc<tokio::sync::Notify>>,
}

impl FaultTransport {
    fn enter(&mut self, phase: &'static str) -> crate::ImResult<()> {
        self.phases.borrow_mut().push(phase);
        if self
            .failures
            .front()
            .is_some_and(|(expected, _)| *expected == phase)
        {
            return Err(self.failures.pop_front().unwrap().1);
        }
        Ok(())
    }
}

impl AsyncAuthenticatedRpcTransport for FaultTransport {
    async fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value> {
        self.enter(if method == "attachment.get_download_ticket" {
            "ticket"
        } else {
            "history"
        })?;
        AuthenticatedRpcTransport::authenticated_rpc(&mut self.inner, endpoint, method, params)
    }
}

impl AsyncRawJsonTransport for FaultTransport {
    async fn get_json_url(
        &mut self,
        url: &str,
        headers: BTreeMap<String, String>,
    ) -> crate::ImResult<Value> {
        self.enter("discovery")?;
        if let Some(ready) = &self.stall_discovery {
            ready.notify_one();
            std::future::pending::<()>().await;
        }
        RawJsonTransport::get_json_url(&mut self.inner, url, headers)
    }
}

impl AsyncAttachmentObjectTransport for FaultTransport {
    async fn put_attachment_object(
        &mut self,
        _: &str,
        _: BTreeMap<String, String>,
        _: Vec<u8>,
    ) -> crate::ImResult<()> {
        unreachable!()
    }
    async fn get_attachment_object(
        &mut self,
        _: &str,
        _: &str,
    ) -> crate::ImResult<AttachmentObjectResponse> {
        unreachable!()
    }
    async fn get_attachment_object_stream_from(
        &mut self,
        uri: &str,
        ticket: &str,
        offset: u64,
    ) -> crate::ImResult<AsyncAttachmentObjectResponse> {
        self.enter("object")?;
        AsyncAttachmentObjectTransport::get_attachment_object_stream_from(
            &mut self.inner,
            uri,
            ticket,
            offset,
        )
        .await
    }
}

fn fault_transport(failures: VecDeque<(&'static str, crate::ImError)>) -> FaultTransport {
    FaultTransport {
        inner: RecordingTransport {
            calls: Rc::new(RefCell::new(vec![])),
        },
        failures,
        phases: Rc::new(RefCell::new(vec![])),
        stall_discovery: None,
    }
}

fn input(destination: crate::attachments::AttachmentDestination) -> AttachmentDownloadInput {
    AttachmentDownloadInput {
        request: crate::attachments::DownloadAttachmentRequest {
            thread: crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:web:example.com:bob", "").unwrap(),
            ),
            message_id: crate::ids::MessageId::parse("msg-attachment-1").unwrap(),
            attachment_id: Some("att-1".into()),
            destination,
            overwrite: false,
        },
        resolved_peer_did: None,
    }
}

fn network() -> crate::ImError {
    crate::ImError::TransportUnavailable {
        detail: "test connection interrupted".into(),
    }
}

#[tokio::test]
async fn attachment_preparation_and_stream_share_retry_budget() {
    for local in [false, true] {
        for exhausted in [false, true] {
            let fixture = Fixture::new();
            let client = fixture.client();
            let mut faults = VecDeque::from([
                ("discovery", network()),
                ("ticket", network()),
                ("object", network()),
            ]);
            if exhausted {
                faults.push_back(("object", network()));
            }
            let transport = fault_transport(faults);
            let phases = transport.phases.clone();
            let destination = if local {
                crate::attachments::AttachmentDestination::LocalFile(
                    fixture.root.join("download.txt"),
                )
            } else {
                crate::attachments::AttachmentDestination::Memory
            };
            let result = AttachmentDownloadRuntime {
                client: &client,
                session_provider: ReadySessionProvider {
                    scopes: Rc::new(RefCell::new(vec![])),
                },
                transport,
            }
            .download_async(input(destination))
            .await;
            assert_eq!(result.is_err(), exhausted, "local={local}");
            let phases = phases.borrow();
            assert_eq!(phases.iter().filter(|p| **p == "discovery").count(), 2);
            assert_eq!(phases.iter().filter(|p| **p == "object").count(), 2);
            assert_eq!(phases.iter().filter(|p| **p == "ticket").count(), 3);
        }
    }
}

#[tokio::test]
async fn attachment_preparation_failure_is_typed_and_authorization_is_not_retried() {
    for (phase, error, retries) in [
        ("discovery", network(), 4),
        ("ticket", crate::ImError::PermissionDenied, 1),
        (
            "history",
            crate::ImError::MessageNotFound {
                message_id: "missing".into(),
            },
            1,
        ),
    ] {
        let fixture = Fixture::new();
        let client = fixture.client();
        let transport = fault_transport(VecDeque::from(vec![(phase, error); retries]));
        let phases = transport.phases.clone();
        let result = AttachmentDownloadRuntime {
            client: &client,
            session_provider: ReadySessionProvider {
                scopes: Rc::new(RefCell::new(vec![])),
            },
            transport,
        }
        .download_async(input(crate::attachments::AttachmentDestination::Memory))
        .await;
        let error = result.err().unwrap();
        assert!(
            matches!(&error, crate::ImError::AttachmentPreparation { stage, retryable, .. }
            if stage.as_str() == phase && *retryable == (retries > 1)),
            "{error:?}"
        );
        assert_eq!(
            phases.borrow().iter().filter(|p| **p == phase).count(),
            retries
        );
        assert!(!phases.borrow().contains(&"object"));
    }
}

#[tokio::test]
async fn attachment_cancel_interrupts_stalled_preparation_without_object_request() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let ready = std::sync::Arc::new(tokio::sync::Notify::new());
    let mut transport = fault_transport(VecDeque::new());
    transport.stall_discovery = Some(ready.clone());
    let phases = transport.phases.clone();
    let output = fixture.root.join("cancelled.txt");
    let runtime = AttachmentDownloadRuntime {
        client: &client,
        session_provider: ReadySessionProvider {
            scopes: Rc::new(RefCell::new(vec![])),
        },
        transport,
    };
    let (result, ()) = tokio::join!(
        runtime.download_async(input(crate::attachments::AttachmentDestination::LocalFile(
            output.clone()
        ))),
        async {
            ready.notified().await;
            assert!(crate::internal::attachment_runtime::cancellation::cancel(
                &output
            ));
        }
    );
    assert!(matches!(
        result,
        Err(crate::ImError::AttachmentTransfer {
            failure: crate::AttachmentTransferFailure::Cancelled,
            ..
        })
    ));
    assert!(!output.exists());
    assert!(!phases.borrow().contains(&"object"));
}
