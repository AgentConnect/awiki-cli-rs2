use serde_json::Value;

use crate::internal::auth::session::{AsyncSessionProvider, SessionProvider};
use crate::internal::transport::{AsyncAuthenticatedRestTransport, AuthenticatedRestTransport};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ContactBindingRuntimeResult {
    pub(crate) sdk_result: crate::identity::ContactBindingResult,
    pub(crate) raw_status: Option<Value>,
    pub(crate) raw_send: Option<Value>,
}

pub(crate) struct ContactBindingRuntime<'a, P, T> {
    _client: &'a crate::core::ImClient,
    session_provider: P,
    transport: T,
}

impl<'a, P, T> ContactBindingRuntime<'a, P, T> {
    pub(crate) fn new(
        client: &'a crate::core::ImClient,
        session_provider: P,
        transport: T,
    ) -> Self {
        Self {
            _client: client,
            session_provider,
            transport,
        }
    }
}

impl<'a, P, T> ContactBindingRuntime<'a, P, T>
where
    P: SessionProvider,
    T: AuthenticatedRestTransport,
{
    pub(crate) fn bind_contact(
        mut self,
        request: crate::identity::ContactBindingRequest,
    ) -> crate::ImResult<ContactBindingRuntimeResult> {
        validate_request(&request)?;
        self.session_provider
            .ensure_session(crate::auth::AuthScope::UserProfile)?;
        match request.method {
            crate::identity::ContactBindingMethod::Phone { phone, otp } => {
                self.bind_phone(phone, otp)
            }
            crate::identity::ContactBindingMethod::Email { email } => {
                self.bind_email(email, request.wait_for_email_verification)
            }
        }
    }

    pub(crate) fn email_status(
        mut self,
        email: String,
    ) -> crate::ImResult<ContactBindingRuntimeResult> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::UserProfile)?;
        let email = crate::internal::identity_wire::required_normalized_email(&email)?;
        let status = self.email_status_value(&email)?;
        let state = if status.as_ref().is_some_and(email_verified) {
            crate::identity::ContactBindingState::Completed
        } else {
            crate::identity::ContactBindingState::Pending
        };
        let sdk_result = binding_result(
            crate::identity::ContactBindingMethodKind::Email,
            email,
            state,
            status.clone(),
        );
        Ok(ContactBindingRuntimeResult {
            sdk_result,
            raw_status: status,
            raw_send: None,
        })
    }

    fn bind_phone(
        &mut self,
        phone: String,
        otp: Option<String>,
    ) -> crate::ImResult<ContactBindingRuntimeResult> {
        let phone = crate::internal::identity_wire::normalize_phone(&phone)?;
        if let Some(otp) = otp.filter(|otp| !otp.trim().is_empty()) {
            let call = crate::internal::identity_wire::bind::build_phone_bind_verify_rest_call(
                &phone, &otp,
            )?;
            let raw =
                self.transport
                    .authenticated_rest_post(call.endpoint, call.method, call.body)?;
            let sdk_result = binding_result(
                crate::identity::ContactBindingMethodKind::Phone,
                phone,
                crate::identity::ContactBindingState::Completed,
                Some(raw.clone()),
            );
            return Ok(ContactBindingRuntimeResult {
                sdk_result,
                raw_status: None,
                raw_send: Some(raw),
            });
        }
        let call = crate::internal::identity_wire::bind::build_phone_bind_send_rest_call(&phone)?;
        let raw = self
            .transport
            .authenticated_rest_post(call.endpoint, call.method, call.body)?;
        let sdk_result = binding_result(
            crate::identity::ContactBindingMethodKind::Phone,
            phone,
            crate::identity::ContactBindingState::OtpSent,
            Some(raw.clone()),
        );
        Ok(ContactBindingRuntimeResult {
            sdk_result,
            raw_status: None,
            raw_send: Some(raw),
        })
    }

    fn bind_email(
        &mut self,
        email: String,
        wait_for_email_verification: bool,
    ) -> crate::ImResult<ContactBindingRuntimeResult> {
        let email = crate::internal::identity_wire::required_normalized_email(&email)?;
        let status = self.email_status_value(&email)?;
        if status.as_ref().is_some_and(email_verified) {
            let sdk_result = binding_result(
                crate::identity::ContactBindingMethodKind::Email,
                email,
                crate::identity::ContactBindingState::Completed,
                status.clone(),
            );
            return Ok(ContactBindingRuntimeResult {
                sdk_result,
                raw_status: status,
                raw_send: None,
            });
        }

        let send_call =
            crate::internal::identity_wire::bind::build_email_send_rest_call(&email, None, true)?;
        let send = self.transport.authenticated_rest_post(
            send_call.endpoint,
            send_call.method,
            send_call.body,
        )?;

        if wait_for_email_verification {
            let sdk_result = binding_result(
                crate::identity::ContactBindingMethodKind::Email,
                email,
                crate::identity::ContactBindingState::Pending,
                Some(send.clone()),
            );
            return Ok(ContactBindingRuntimeResult {
                sdk_result,
                raw_status: status,
                raw_send: Some(send),
            });
        }

        let sdk_result = binding_result(
            crate::identity::ContactBindingMethodKind::Email,
            email,
            crate::identity::ContactBindingState::EmailSent,
            Some(send.clone()),
        );
        Ok(ContactBindingRuntimeResult {
            sdk_result,
            raw_status: status,
            raw_send: Some(send),
        })
    }

    fn email_status_value(&mut self, email: &str) -> crate::ImResult<Option<Value>> {
        let status_call =
            crate::internal::identity_wire::bind::build_email_status_rest_call(email, None, true)?;
        match self.transport.authenticated_rest_get(
            status_call.endpoint,
            status_call.method,
            &status_call.query,
        ) {
            Ok(status) => Ok(Some(status)),
            Err(crate::ImError::Service {
                status_code: Some(404),
                ..
            }) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl<'a, P, T> ContactBindingRuntime<'a, P, T>
where
    P: AsyncSessionProvider,
    T: AsyncAuthenticatedRestTransport,
{
    pub(crate) async fn bind_contact_async(
        mut self,
        request: crate::identity::ContactBindingRequest,
    ) -> crate::ImResult<ContactBindingRuntimeResult> {
        validate_request(&request)?;
        self.session_provider
            .ensure_session(crate::auth::AuthScope::UserProfile)
            .await?;
        match request.method {
            crate::identity::ContactBindingMethod::Phone { phone, otp } => {
                self.bind_phone_async(phone, otp).await
            }
            crate::identity::ContactBindingMethod::Email { email } => {
                self.bind_email_async(email, request.wait_for_email_verification)
                    .await
            }
        }
    }

    pub(crate) async fn email_status_async(
        mut self,
        email: String,
    ) -> crate::ImResult<ContactBindingRuntimeResult> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::UserProfile)
            .await?;
        let email = crate::internal::identity_wire::required_normalized_email(&email)?;
        let status = self.email_status_value_async(&email).await?;
        let state = if status.as_ref().is_some_and(email_verified) {
            crate::identity::ContactBindingState::Completed
        } else {
            crate::identity::ContactBindingState::Pending
        };
        let sdk_result = binding_result(
            crate::identity::ContactBindingMethodKind::Email,
            email,
            state,
            status.clone(),
        );
        Ok(ContactBindingRuntimeResult {
            sdk_result,
            raw_status: status,
            raw_send: None,
        })
    }

    async fn bind_phone_async(
        &mut self,
        phone: String,
        otp: Option<String>,
    ) -> crate::ImResult<ContactBindingRuntimeResult> {
        let phone = crate::internal::identity_wire::normalize_phone(&phone)?;
        if let Some(otp) = otp.filter(|otp| !otp.trim().is_empty()) {
            let call = crate::internal::identity_wire::bind::build_phone_bind_verify_rest_call(
                &phone, &otp,
            )?;
            let raw = self
                .transport
                .authenticated_rest_post(call.endpoint, call.method, call.body)
                .await?;
            let sdk_result = binding_result(
                crate::identity::ContactBindingMethodKind::Phone,
                phone,
                crate::identity::ContactBindingState::Completed,
                Some(raw.clone()),
            );
            return Ok(ContactBindingRuntimeResult {
                sdk_result,
                raw_status: None,
                raw_send: Some(raw),
            });
        }
        let call = crate::internal::identity_wire::bind::build_phone_bind_send_rest_call(&phone)?;
        let raw = self
            .transport
            .authenticated_rest_post(call.endpoint, call.method, call.body)
            .await?;
        let sdk_result = binding_result(
            crate::identity::ContactBindingMethodKind::Phone,
            phone,
            crate::identity::ContactBindingState::OtpSent,
            Some(raw.clone()),
        );
        Ok(ContactBindingRuntimeResult {
            sdk_result,
            raw_status: None,
            raw_send: Some(raw),
        })
    }

    async fn bind_email_async(
        &mut self,
        email: String,
        wait_for_email_verification: bool,
    ) -> crate::ImResult<ContactBindingRuntimeResult> {
        let email = crate::internal::identity_wire::required_normalized_email(&email)?;
        let status = self.email_status_value_async(&email).await?;
        if status.as_ref().is_some_and(email_verified) {
            let sdk_result = binding_result(
                crate::identity::ContactBindingMethodKind::Email,
                email,
                crate::identity::ContactBindingState::Completed,
                status.clone(),
            );
            return Ok(ContactBindingRuntimeResult {
                sdk_result,
                raw_status: status,
                raw_send: None,
            });
        }

        let send_call =
            crate::internal::identity_wire::bind::build_email_send_rest_call(&email, None, true)?;
        let send = self
            .transport
            .authenticated_rest_post(send_call.endpoint, send_call.method, send_call.body)
            .await?;

        if wait_for_email_verification {
            let sdk_result = binding_result(
                crate::identity::ContactBindingMethodKind::Email,
                email,
                crate::identity::ContactBindingState::Pending,
                Some(send.clone()),
            );
            return Ok(ContactBindingRuntimeResult {
                sdk_result,
                raw_status: status,
                raw_send: Some(send),
            });
        }

        let sdk_result = binding_result(
            crate::identity::ContactBindingMethodKind::Email,
            email,
            crate::identity::ContactBindingState::EmailSent,
            Some(send.clone()),
        );
        Ok(ContactBindingRuntimeResult {
            sdk_result,
            raw_status: status,
            raw_send: Some(send),
        })
    }

    async fn email_status_value_async(&mut self, email: &str) -> crate::ImResult<Option<Value>> {
        let status_call =
            crate::internal::identity_wire::bind::build_email_status_rest_call(email, None, true)?;
        match self
            .transport
            .authenticated_rest_get(status_call.endpoint, status_call.method, &status_call.query)
            .await
        {
            Ok(status) => Ok(Some(status)),
            Err(crate::ImError::Service {
                status_code: Some(404),
                ..
            }) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

pub(crate) fn validate_request(
    request: &crate::identity::ContactBindingRequest,
) -> crate::ImResult<()> {
    match &request.method {
        crate::identity::ContactBindingMethod::Phone { phone, .. } => {
            crate::internal::identity_wire::normalize_phone(phone)?;
        }
        crate::identity::ContactBindingMethod::Email { email } => {
            crate::internal::identity_wire::required_normalized_email(email)?;
        }
    }
    Ok(())
}

pub(crate) fn email_verified(value: &Value) -> bool {
    value
        .get("verified")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn binding_result(
    method: crate::identity::ContactBindingMethodKind,
    target: String,
    state: crate::identity::ContactBindingState,
    raw: Option<Value>,
) -> crate::identity::ContactBindingResult {
    crate::identity::ContactBindingResult::with_raw_response(method, target, state, raw, Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn identity_bind_status_projection_maps_verified_flag() {
        assert!(email_verified(&serde_json::json!({ "verified": true })));
        assert!(!email_verified(&serde_json::json!({ "verified": false })));
        assert!(!email_verified(&serde_json::json!({})));
    }

    #[test]
    fn identity_bind_phone_send_and_verify_choose_expected_rest_calls() {
        let client = fixture_client();
        let sent = ContactBindingRuntime::new(
            &client,
            TestSession,
            TestTransport {
                posts: vec![serde_json::json!({"sent": true})],
                ..TestTransport::default()
            },
        )
        .bind_contact(crate::identity::ContactBindingRequest {
            method: crate::identity::ContactBindingMethod::Phone {
                phone: "13800138000".to_string(),
                otp: None,
            },
            wait_for_email_verification: false,
        })
        .unwrap();
        assert_eq!(
            sent.sdk_result.state,
            crate::identity::ContactBindingState::OtpSent
        );
        assert_eq!(sent.sdk_result.target, "+8613800138000");
        assert_eq!(sent.raw_send.unwrap()["sent"], true);

        let verified = ContactBindingRuntime::new(
            &client,
            TestSession,
            TestTransport {
                posts: vec![serde_json::json!({"bound": true})],
                ..TestTransport::default()
            },
        )
        .bind_contact(crate::identity::ContactBindingRequest {
            method: crate::identity::ContactBindingMethod::Phone {
                phone: "+15551234567".to_string(),
                otp: Some(" 123 456 ".to_string()),
            },
            wait_for_email_verification: false,
        })
        .unwrap();
        assert_eq!(
            verified.sdk_result.state,
            crate::identity::ContactBindingState::Completed
        );
        assert_eq!(verified.sdk_result.target, "+15551234567");
        assert_eq!(verified.raw_send.unwrap()["bound"], true);
    }

    #[test]
    fn identity_bind_email_maps_sent_pending_and_completed() {
        let client = fixture_client();
        let sent = ContactBindingRuntime::new(
            &client,
            TestSession,
            TestTransport {
                gets: vec![Err(crate::ImError::Service {
                    status_code: Some(404),
                    code: None,
                    message: "not found".to_string(),
                })],
                posts: vec![serde_json::json!({"sent": true})],
            },
        )
        .bind_contact(crate::identity::ContactBindingRequest {
            method: crate::identity::ContactBindingMethod::Email {
                email: " Alice@Example.COM ".to_string(),
            },
            wait_for_email_verification: false,
        })
        .unwrap();
        assert_eq!(
            sent.sdk_result.state,
            crate::identity::ContactBindingState::EmailSent
        );
        assert_eq!(sent.sdk_result.target, "alice@example.com");

        let pending = ContactBindingRuntime::new(
            &client,
            TestSession,
            TestTransport {
                gets: vec![Ok(serde_json::json!({"verified": false}))],
                posts: vec![serde_json::json!({"sent": true})],
            },
        )
        .bind_contact(crate::identity::ContactBindingRequest {
            method: crate::identity::ContactBindingMethod::Email {
                email: "alice@example.com".to_string(),
            },
            wait_for_email_verification: true,
        })
        .unwrap();
        assert_eq!(
            pending.sdk_result.state,
            crate::identity::ContactBindingState::Pending
        );
        assert_eq!(pending.raw_send.unwrap()["sent"], true);

        let completed = ContactBindingRuntime::new(
            &client,
            TestSession,
            TestTransport {
                gets: vec![Ok(serde_json::json!({"verified": true}))],
                posts: Vec::new(),
            },
        )
        .bind_contact(crate::identity::ContactBindingRequest {
            method: crate::identity::ContactBindingMethod::Email {
                email: "alice@example.com".to_string(),
            },
            wait_for_email_verification: true,
        })
        .unwrap();
        assert_eq!(
            completed.sdk_result.state,
            crate::identity::ContactBindingState::Completed
        );
    }

    struct TestSession;

    impl crate::internal::auth::session::SessionProvider for TestSession {
        fn ensure_session(
            &self,
            scope: crate::auth::AuthScope,
        ) -> crate::ImResult<crate::auth::SessionBundle> {
            Ok(crate::auth::SessionBundle {
                subject: crate::ids::Did::parse("did:example:alice").unwrap(),
                scope,
                expires_at: None,
                refreshed: false,
                bearer_token: None,
            })
        }

        fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
            unimplemented!()
        }

        fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
            unimplemented!()
        }
    }

    #[derive(Default)]
    struct TestTransport {
        gets: Vec<crate::ImResult<serde_json::Value>>,
        posts: Vec<serde_json::Value>,
    }

    impl crate::internal::transport::AuthenticatedRestTransport for TestTransport {
        fn authenticated_rest_post(
            &mut self,
            _endpoint: &str,
            _method: &str,
            _body: serde_json::Value,
        ) -> crate::ImResult<serde_json::Value> {
            Ok(self.posts.remove(0))
        }

        fn authenticated_rest_get(
            &mut self,
            _endpoint: &str,
            _method: &str,
            _query: &BTreeMap<String, String>,
        ) -> crate::ImResult<serde_json::Value> {
            self.gets.remove(0)
        }
    }

    fn fixture_client() -> crate::core::ImClient {
        let root = tempfile::TempDir::new().unwrap();
        let identities = root.path().join("identities");
        std::fs::create_dir_all(identities.join("alice")).unwrap();
        std::fs::write(identities.join("default"), "alice\n").unwrap();
        std::fs::write(
            identities.join("registry.json"),
            r#"{
              "default_identity": "alice",
              "identities": [{
                "id": "alice-id",
                "did": "did:example:alice",
                "handle": "alice.awiki.test",
                "display_name": "Alice",
                "local_alias": "alice",
                "ready_for_auth": true,
                "ready_for_messaging": true,
                "missing": []
              }]
            }"#,
        )
        .unwrap();
        std::fs::write(
            identities.join("alice").join("did.json"),
            r#"{"id":"did:example:alice","controller":"did:example:alice"}"#,
        )
        .unwrap();
        std::fs::write(identities.join("alice").join("private.key"), "key\n").unwrap();
        std::fs::write(
            identities.join("alice").join("auth.json"),
            r#"{"jwt_token":"token"}"#,
        )
        .unwrap();
        let core = crate::core::ImCore::new(
            crate::config::ImCoreConfig {
                service_base_url: crate::config::ServiceEndpoint::parse("https://example.test")
                    .unwrap(),
                did_domain: "awiki.test".to_string(),
                user_service_endpoint: None,
                message_service_endpoint: None,
                mail_service_endpoint: None,
                anp_service_endpoint: None,
                anp_service_did: None,
                ca_bundle: None,
                transport_policy: crate::config::MessageTransportPolicy::HttpOnly,
            },
            crate::paths::ImCorePaths {
                identities: crate::paths::IdentityRegistryPaths {
                    identity_root_dir: identities.clone(),
                    registry_path: identities.join("registry.json"),
                    default_identity_path: Some(identities.join("default")),
                },
                local_state: crate::paths::LocalStatePaths {
                    sqlite_path: root.path().join("local").join("im.sqlite"),
                },
                runtime: crate::paths::RuntimePaths {
                    cache_dir: root.path().join("cache"),
                    temp_dir: root.path().join("tmp"),
                },
            },
        )
        .unwrap();
        core.client(crate::identity::IdentitySelector::LocalAlias(
            "alice".to_string(),
        ))
        .unwrap()
    }
}
