//! One-time email Join factors. The grant never crosses the public output boundary.
use crate::internal::{
    identity_wire,
    transport::{AsyncRestTransport, CorePlainTransport},
};
use serde_json::{json, Value};

pub struct EmailDeviceJoinRequest {
    pub email: String,
    pub code: String,
    pub full_handle: String,
    pub did: crate::ids::Did,
    pub operation_id: String,
    pub ttl_seconds: u64,
}

impl std::fmt::Debug for EmailDeviceJoinRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EmailDeviceJoinRequest(<redacted>)")
    }
}

pub(super) async fn send(
    core: &crate::core::ImCore,
    email: &str,
    handle: &str,
) -> crate::ImResult<()> {
    let mut transport = CorePlainTransport::new(core);
    send_with(&mut transport, email, handle).await
}

async fn send_with(
    transport: &mut impl AsyncRestTransport,
    email: &str,
    handle: &str,
) -> crate::ImResult<()> {
    transport
        .rest_post(
            "/user-service/v1/auth/email-codes",
            "POST",
            factor_body(email, handle)?,
        )
        .await?;
    Ok(())
}

pub(super) async fn begin(
    core: &crate::core::ImCore,
    request: EmailDeviceJoinRequest,
) -> crate::ImResult<super::DeviceJoinProgress> {
    let mut body = factor_body(&request.email, &request.full_handle)?;
    let operation_id =
        identity_wire::handle_recovery::validate_operation_id(&request.operation_id)?;
    if request.code.len() != 6
        || !request.code.bytes().all(|byte| byte.is_ascii_digit())
        || !(60..=3600).contains(&request.ttl_seconds)
    {
        return Err(crate::ImError::invalid_input(
            None,
            "invalid Join factor or lifetime",
        ));
    }
    body["provider"] = json!("email");
    body["code"] = json!(request.code);
    body["idempotency_scope"] = json!(operation_id);
    let raw = CorePlainTransport::new(core)
        .rest_post(
            identity_wire::ACCOUNT_VERIFICATION_EXCHANGE_ENDPOINT,
            "POST",
            body,
        )
        .await?;
    let verified = identity_wire::handle_recovery::parse_account_verification_result(raw)?;
    if verified
        .did
        .as_deref()
        .is_some_and(|did| did != request.did.as_str())
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let grant = super::DeviceJoinAccountVerificationGrant::from_bytes(
        verified.account_verification_token.expose_secret().to_vec(),
    )?;
    core.device_join()
        .begin_new_device_join(super::DeviceJoinBeginRequest {
            operation_id,
            did: request.did,
            ttl_seconds: request.ttl_seconds,
            account_verification_grant: grant,
        })
        .await
}

fn factor_body(email: &str, handle: &str) -> crate::ImResult<Value> {
    let email = email.trim().to_lowercase();
    if email.len() > 254
        || email.chars().any(char::is_whitespace)
        || email.split('@').count() != 2
        || email.starts_with('@')
        || email.ends_with('@')
    {
        return Err(crate::ImError::invalid_input(
            Some("email".into()),
            "invalid email address",
        ));
    }
    let handle = identity_wire::handle_recovery::canonical_handle(handle)?;
    Ok(
        json!({"email":email,"purpose":"awiki.device.join.v1","target_handle":handle.local_part,"target_handle_domain":handle.domain}),
    )
}

#[cfg(test)]
#[path = "join_factor_tests.rs"]
mod tests;
