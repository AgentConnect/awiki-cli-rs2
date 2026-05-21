use serde_json::json;

pub(crate) fn build_register_rpc_call(
    params: super::RegisterRpcParams,
) -> crate::ImResult<super::RpcCall> {
    let handle = super::required_trimmed(&params.handle, "handle")?;
    let mut payload = serde_json::Map::new();
    payload.insert("did_document".to_string(), params.did_document);
    payload.insert("handle".to_string(), serde_json::Value::String(handle));
    if let Some(phone) = params.phone {
        if !phone.trim().is_empty() {
            payload.insert(
                "phone".to_string(),
                serde_json::Value::String(super::normalize_phone(&phone)?),
            );
            payload.insert(
                "otp_code".to_string(),
                serde_json::Value::String(super::sanitize_otp(
                    params.otp_code.as_deref().unwrap_or_default(),
                )),
            );
        }
    }
    if let Some(email) = params.email {
        let email = super::normalize_email(&email);
        if !email.is_empty() {
            payload.insert("email".to_string(), serde_json::Value::String(email));
        }
    }
    if !params.invite_code.is_empty() {
        payload.insert(
            "invite_code".to_string(),
            serde_json::Value::String(params.invite_code),
        );
    }
    Ok(super::rpc_call(
        super::DID_AUTH_RPC_ENDPOINT,
        "register",
        super::TransportProfile::RpcDefault,
        serde_json::Value::Object(payload),
    ))
}

pub(crate) fn build_recover_handle_rpc_call(
    params: super::RecoverHandleRpcParams,
) -> crate::ImResult<super::RpcCall> {
    let handle = super::required_trimmed(&params.handle, "handle")?;
    Ok(super::rpc_call(
        super::DID_AUTH_RPC_ENDPOINT,
        "recover_handle",
        super::TransportProfile::RpcDefault,
        json!({
            "did_document": params.did_document,
            "handle": handle,
            "phone": super::normalize_phone(&params.phone)?,
            "otp_code": super::sanitize_otp(&params.otp_code),
        }),
    ))
}
