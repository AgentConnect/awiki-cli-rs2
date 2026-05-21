use serde_json::Value;

use crate::internal::transport::RpcTransport;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct IdentityRecoveryRuntimeResult {
    pub(crate) sdk_result: crate::identity::RecoverHandleResult,
    pub(crate) raw: Value,
}

pub(crate) struct IdentityRecoveryRuntime<T> {
    transport: T,
}

impl<T> IdentityRecoveryRuntime<T>
where
    T: RpcTransport,
{
    pub(crate) fn new(transport: T) -> Self {
        Self { transport }
    }

    pub(crate) fn recover_handle(
        mut self,
        request: crate::identity::RecoverHandleRequest,
    ) -> crate::ImResult<IdentityRecoveryRuntimeResult> {
        validate_request(&request)?;
        let phone = crate::internal::identity_wire::normalize_phone(&request.phone)?;
        if let Some(otp) = request
            .otp
            .as_deref()
            .map(str::trim)
            .filter(|otp| !otp.is_empty())
            .map(str::to_string)
        {
            return self.recover_with_otp(request, phone, otp);
        }
        let call = crate::internal::identity_wire::directory::build_send_otp_rpc_call(&phone)?;
        let raw = self
            .transport
            .rpc(call.endpoint, call.method, call.params.clone())?;
        let sdk_result = crate::identity::RecoverHandleResult {
            handle: request.handle,
            phone,
            state: crate::identity::RecoverHandleState::OtpSent,
            recovered_identity: None,
            raw: Some(raw.clone()),
            warnings: Vec::new(),
        };
        Ok(IdentityRecoveryRuntimeResult { sdk_result, raw })
    }

    fn recover_with_otp(
        &mut self,
        request: crate::identity::RecoverHandleRequest,
        phone: String,
        otp: String,
    ) -> crate::ImResult<IdentityRecoveryRuntimeResult> {
        let generated = request.generated_identity.as_ref().ok_or_else(|| {
            crate::ImError::invalid_input(
                Some("generated_identity".to_string()),
                "generated identity is required when otp is provided",
            )
        })?;
        let call = crate::internal::identity_wire::recovery::build_recover_handle_rpc_call(
            crate::internal::identity_wire::RecoverHandleRpcParams {
                did_document: generated.did_document.clone(),
                handle: request.handle.as_str().to_string(),
                phone: phone.clone(),
                otp_code: otp,
            },
        )?;
        let raw = self
            .transport
            .rpc(call.endpoint, call.method, call.params.clone())?;
        let identity = recovered_identity_summary(&request, &generated, &raw)?;
        let recovered_identity = crate::identity::RecoveredIdentity {
            identity,
            user_id: raw
                .get("user_id")
                .and_then(Value::as_str)
                .map(str::to_string),
            access_token_present: raw
                .get("access_token")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty()),
        };
        let sdk_result = crate::identity::RecoverHandleResult {
            handle: request.handle,
            phone,
            state: crate::identity::RecoverHandleState::Recovered,
            recovered_identity: Some(recovered_identity),
            raw: Some(raw.clone()),
            warnings: Vec::new(),
        };
        Ok(IdentityRecoveryRuntimeResult { sdk_result, raw })
    }
}

pub(crate) fn validate_request(
    request: &crate::identity::RecoverHandleRequest,
) -> crate::ImResult<()> {
    crate::internal::identity_wire::required_trimmed(request.handle.as_str(), "handle")?;
    crate::internal::identity_wire::normalize_phone(&request.phone)?;
    if request
        .otp
        .as_deref()
        .is_some_and(|otp| !otp.trim().is_empty())
    {
        let generated = request.generated_identity.as_ref().ok_or_else(|| {
            crate::ImError::invalid_input(
                Some("generated_identity".to_string()),
                "generated identity is required when otp is provided",
            )
        })?;
        if generated.unique_id.trim().is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("generated_identity.unique_id".to_string()),
                "generated identity unique_id must not be empty",
            ));
        }
        if !generated.did_document.is_object() {
            return Err(crate::ImError::invalid_input(
                Some("generated_identity.did_document".to_string()),
                "generated identity did_document must be an object",
            ));
        }
    }
    Ok(())
}

fn recovered_identity_summary(
    request: &crate::identity::RecoverHandleRequest,
    generated: &crate::identity::RecoverGeneratedIdentity,
    raw: &Value,
) -> crate::ImResult<crate::identity::IdentitySummary> {
    let did = raw
        .get("did")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| generated.did.as_str());
    let handle = raw
        .get("full_handle")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| request.handle.as_str());
    let local_alias = local_part(handle).to_string();
    Ok(crate::identity::IdentitySummary {
        id: crate::ids::IdentityId::parse(&generated.unique_id)?,
        did: crate::ids::Did::parse(did)?,
        handle: Some(crate::ids::Handle::parse(handle, "")?),
        display_name: Some(
            raw.get("handle")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| local_part(handle))
                .to_string(),
        ),
        local_alias: Some(local_alias),
        device_id: None,
        is_default: false,
        readiness: crate::identity::IdentityReadiness {
            ready_for_auth: true,
            ready_for_messaging: true,
            missing: Vec::new(),
        },
    })
}

fn local_part(handle: &str) -> &str {
    handle
        .trim_start_matches('@')
        .split('.')
        .next()
        .unwrap_or(handle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_recovery_without_otp_sends_recover_otp() {
        let result = IdentityRecoveryRuntime::new(TestTransport {
            responses: vec![serde_json::json!({"sent": true})],
            calls: Vec::new(),
        })
        .recover_handle(crate::identity::RecoverHandleRequest {
            handle: crate::ids::Handle::parse("alice.awiki.test", "").unwrap(),
            phone: "13800138000".to_string(),
            otp: None,
            generated_identity: None,
        })
        .unwrap();

        assert_eq!(
            result.sdk_result.state,
            crate::identity::RecoverHandleState::OtpSent
        );
        assert_eq!(result.sdk_result.phone, "+8613800138000");
        assert_eq!(result.raw["sent"], true);
    }

    #[test]
    fn identity_recovery_with_otp_calls_recover_handle_and_maps_summary() {
        let generated = crate::identity::RecoverGeneratedIdentity {
            did: crate::ids::Did::parse("did:wba:awiki.test:alice:e1_generated").unwrap(),
            unique_id: "e1_generated".to_string(),
            did_document: serde_json::json!({
                "id": "did:wba:awiki.test:alice:e1_generated"
            }),
        };
        let result = IdentityRecoveryRuntime::new(TestTransport {
            responses: vec![serde_json::json!({
                "did": "did:wba:awiki.test:alice:e1_recovered",
                "user_id": "user-alice",
                "handle": "alice",
                "full_handle": "alice.awiki.test",
                "access_token": "jwt-recover"
            })],
            calls: Vec::new(),
        })
        .recover_handle(crate::identity::RecoverHandleRequest {
            handle: crate::ids::Handle::parse("alice.awiki.test", "").unwrap(),
            phone: "+15551234567".to_string(),
            otp: Some(" 12 34 56 ".to_string()),
            generated_identity: Some(generated),
        })
        .unwrap();

        let recovered = result.sdk_result.recovered_identity.unwrap();
        assert_eq!(
            result.sdk_result.state,
            crate::identity::RecoverHandleState::Recovered
        );
        assert_eq!(
            recovered.identity.did.as_str(),
            "did:wba:awiki.test:alice:e1_recovered"
        );
        assert_eq!(
            recovered.identity.handle.unwrap().as_str(),
            "alice.awiki.test"
        );
        assert_eq!(recovered.user_id.as_deref(), Some("user-alice"));
        assert!(recovered.access_token_present);
    }

    struct TestTransport {
        responses: Vec<serde_json::Value>,
        calls: Vec<(String, String, serde_json::Value)>,
    }

    impl crate::internal::transport::RpcTransport for TestTransport {
        fn rpc(
            &mut self,
            endpoint: &str,
            method: &str,
            params: serde_json::Value,
        ) -> crate::ImResult<serde_json::Value> {
            self.calls
                .push((endpoint.to_string(), method.to_string(), params));
            Ok(self.responses.remove(0))
        }
    }
}
