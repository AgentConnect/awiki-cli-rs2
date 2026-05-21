use serde_json::json;

pub(crate) fn build_handle_lookup_by_did_rpc_call(did: &str) -> crate::ImResult<super::RpcCall> {
    let did = super::required_trimmed(did, "did")?;
    Ok(super::rpc_call(
        super::HANDLE_RPC_ENDPOINT,
        "lookup",
        super::TransportProfile::RpcDefault,
        json!({ "did": did }),
    ))
}

pub(crate) fn build_handle_lookup_by_handle_rpc_call(
    handle: &str,
) -> crate::ImResult<super::RpcCall> {
    let handle = super::required_trimmed(handle, "handle")?;
    Ok(super::rpc_call(
        super::HANDLE_RPC_ENDPOINT,
        "lookup",
        super::TransportProfile::RpcDefault,
        json!({ "handle": handle }),
    ))
}

pub(crate) fn build_send_otp_rpc_call(phone: &str) -> crate::ImResult<super::RpcCall> {
    Ok(super::rpc_call(
        super::HANDLE_RPC_ENDPOINT,
        "send_otp",
        super::TransportProfile::RpcDefault,
        json!({ "phone": super::normalize_phone(phone)? }),
    ))
}
