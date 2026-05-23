use serde_json::json;

pub(crate) fn build_follow_rpc_call(target_did: &str) -> crate::ImResult<super::RpcCall> {
    relationship_target_call("follow", target_did)
}

pub(crate) fn build_unfollow_rpc_call(target_did: &str) -> crate::ImResult<super::RpcCall> {
    relationship_target_call("unfollow", target_did)
}

pub(crate) fn build_relationship_status_rpc_call(
    target_did: &str,
) -> crate::ImResult<super::RpcCall> {
    relationship_target_call("get_status", target_did)
}

pub(crate) fn build_followers_rpc_call(limit: u32, offset: u32) -> crate::ImResult<super::RpcCall> {
    relationship_list_call("get_followers", limit, offset)
}

pub(crate) fn build_following_rpc_call(limit: u32, offset: u32) -> crate::ImResult<super::RpcCall> {
    relationship_list_call("get_following", limit, offset)
}

fn relationship_target_call(
    method: &'static str,
    target_did: &str,
) -> crate::ImResult<super::RpcCall> {
    let target_did = super::required_trimmed(target_did, "target_did")?;
    Ok(super::rpc_call(
        super::DID_RELATIONSHIPS_RPC_ENDPOINT,
        method,
        super::TransportProfile::RpcDefault,
        json!({ "target_did": target_did }),
    ))
}

fn relationship_list_call(
    method: &'static str,
    limit: u32,
    offset: u32,
) -> crate::ImResult<super::RpcCall> {
    if limit == 0 {
        return Err(crate::ImError::invalid_input(
            Some("limit".to_string()),
            "limit must be greater than zero",
        ));
    }
    Ok(super::rpc_call(
        super::DID_RELATIONSHIPS_RPC_ENDPOINT,
        method,
        super::TransportProfile::RpcReadHeavy,
        json!({ "limit": limit, "offset": offset }),
    ))
}
