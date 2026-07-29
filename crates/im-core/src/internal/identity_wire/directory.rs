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

pub(crate) fn build_registration_send_otp_rpc_call(
    phone: &str,
    handle: &str,
    domain: &str,
    full_handle: &str,
) -> crate::ImResult<super::RpcCall> {
    let handle = super::required_trimmed(handle, "handle")?;
    let domain = super::required_trimmed(domain, "domain")?;
    let full_handle = super::required_trimmed(full_handle, "full_handle")?;
    if handle != handle.to_ascii_lowercase()
        || domain != domain.to_ascii_lowercase()
        || full_handle != format!("{handle}.{domain}")
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(super::rpc_call(
        super::HANDLE_RPC_ENDPOINT,
        "send_otp",
        super::TransportProfile::RpcDefault,
        json!({
            "phone": super::normalize_phone(phone)?,
            "purpose": "awiki.identity.register.v1",
            "handle": handle,
            "domain": domain,
            "full_handle": full_handle,
        }),
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn registration_otp_uses_closed_v1_target_schema() {
        let call = super::build_registration_send_otp_rpc_call(
            "+8613800138000",
            "alice",
            "example.test",
            "alice.example.test",
        )
        .unwrap();

        assert_eq!(call.method, "send_otp");
        assert_eq!(
            call.params,
            serde_json::json!({
                "phone": "+8613800138000",
                "purpose": "awiki.identity.register.v1",
                "handle": "alice",
                "domain": "example.test",
                "full_handle": "alice.example.test"
            })
        );
    }
}
