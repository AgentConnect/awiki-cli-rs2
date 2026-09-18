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
async fn attachment_preparation_preserves_terminal_errors_and_types_exhausted_retries() {
    for (phase, error, attempts) in [
        ("discovery", network(), 4),
        ("history", network(), 4),
        ("ticket", network(), 4),
        (
            "discovery",
            crate::ImError::InvalidInput {
                field: Some("did".into()),
                message: "unsupported DID method".into(),
            },
            1,
        ),
        ("ticket", crate::ImError::PermissionDenied, 1),
        (
            "history",
            crate::ImError::MessageNotFound {
                message_id: "missing".into(),
            },
            1,
        ),
        (
            "ticket",
            crate::ImError::Service {
                status_code: Some(403),
                code: Some("anp.attachment.access_denied".into()),
                message: "attachment access denied".into(),
                data: None,
            },
            1,
        ),
    ] {
        for local in [false, true] {
            let fixture = Fixture::new();
            let client = fixture.client();
            let transport = fault_transport(VecDeque::from(vec![(phase, error.clone()); attempts]));
            let phases = transport.phases.clone();
            let output = fixture.root.join("failed.txt");
            let destination = if local {
                crate::attachments::AttachmentDestination::LocalFile(output.clone())
            } else {
                crate::attachments::AttachmentDestination::Memory
            };
            let actual = AttachmentDownloadRuntime {
                client: &client,
                session_provider: ReadySessionProvider {
                    scopes: Rc::new(RefCell::new(vec![])),
                },
                transport,
            }
            .download_async(input(destination))
            .await
            .unwrap_err();
            if attempts == 1 {
                assert_eq!(actual, error, "phase={phase}, local={local}");
            } else {
                assert!(
                    matches!(&actual, crate::ImError::AttachmentPreparation { stage, retryable: true, cause }
                    if stage.as_str() == phase && **cause == error),
                    "{actual:?}"
                );
            }
            assert_eq!(
                phases.borrow().iter().filter(|p| **p == phase).count(),
                attempts
            );
            assert!(!phases.borrow().contains(&"object"));
            assert!(!output.exists());
        }
    }
}

struct FailingSessionProvider {
    error: crate::ImError,
    calls: Rc<RefCell<usize>>,
}

impl crate::internal::auth::session::AsyncSessionProvider for FailingSessionProvider {
    async fn ensure_session(
        &self,
        _: crate::auth::AuthScope,
    ) -> crate::ImResult<crate::auth::SessionBundle> {
        *self.calls.borrow_mut() += 1;
        Err(self.error.clone())
    }

    async fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
        unreachable!()
    }

    async fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
        unreachable!()
    }
}

#[tokio::test]
async fn attachment_session_failure_preserves_auth_errors_and_bounds_network_retries() {
    for (error, attempts) in [
        (crate::ImError::AuthRequired, 1),
        (crate::ImError::SessionExpired, 1),
        (network(), 4),
    ] {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(0));
        let transport = fault_transport(VecDeque::new());
        let phases = transport.phases.clone();
        let actual = AttachmentDownloadRuntime {
            client: &client,
            session_provider: FailingSessionProvider {
                error: error.clone(),
                calls: calls.clone(),
            },
            transport,
        }
        .download_async(input(crate::attachments::AttachmentDestination::Memory))
        .await
        .unwrap_err();
        let expected = if attempts == 1 {
            error
        } else {
            crate::ImError::AttachmentPreparation {
                stage: crate::AttachmentPreparationStage::Session,
                retryable: true,
                cause: Box::new(error),
            }
        };
        assert_eq!(actual, expected);
        assert_eq!(*calls.borrow(), attempts);
        assert!(phases.borrow().is_empty());
    }
}

#[tokio::test]
async fn attachment_terminal_error_stays_unwrapped_after_shared_retry_budget_is_spent() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let transport = fault_transport(VecDeque::from([
        ("history", network()),
        ("history", network()),
        ("history", network()),
        ("ticket", crate::ImError::PermissionDenied),
    ]));
    let phases = transport.phases.clone();
    let error = AttachmentDownloadRuntime {
        client: &client,
        session_provider: ReadySessionProvider {
            scopes: Rc::new(RefCell::new(vec![])),
        },
        transport,
    }
    .download_async(input(crate::attachments::AttachmentDestination::Memory))
    .await
    .unwrap_err();
    assert_eq!(error, crate::ImError::PermissionDenied);
    assert_eq!(
        phases.borrow().as_slice(),
        &[
            "history",
            "history",
            "history",
            "history",
            "discovery",
            "ticket"
        ]
    );
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
