pub(crate) fn build_register_rpc_call(
    params: super::RegisterRpcParams,
) -> crate::ImResult<super::RpcCall> {
    let handle = super::required_trimmed(&params.handle, "handle")?;
    let mut payload = serde_json::Map::new();
    payload.insert("did_document".to_string(), params.did_document);
    payload.insert("handle".to_string(), serde_json::Value::String(handle));
    if let Some(name) = params.name {
        let name = name.trim();
        if !name.is_empty() {
            payload.insert(
                "name".to_string(),
                serde_json::Value::String(name.to_string()),
            );
        }
    }
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
    if let Some(operation_id) = params.provision_operation_id {
        payload.insert(
            "provision_operation_id".to_string(),
            serde_json::Value::String(operation_id),
        );
    }
    Ok(super::rpc_call(
        super::DID_AUTH_RPC_ENDPOINT,
        "register",
        super::TransportProfile::RpcDefault,
        serde_json::Value::Object(payload),
    ))
}
