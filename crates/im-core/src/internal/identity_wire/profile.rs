use base64::Engine as _;
use serde_json::{json, Map, Value};

fn avatar_params(request: crate::identity::ClearAvatarRequest) -> crate::ImResult<Value> {
    let request_id = uuid::Uuid::parse_str(&request.request_id).map_err(|_| {
        crate::ImError::invalid_input(
            Some("request_id".to_owned()),
            "avatar request_id must be a UUID",
        )
    })?;
    crate::internal::local_state::sync_v2::validate_decimal(
        "expected_profile_version",
        &request.expected_profile_version,
    )?;
    if request.expected_profile_version.len() > 19
        || (request.expected_profile_version.len() == 19
            && request.expected_profile_version.as_str() > "9223372036854775807")
    {
        return Err(crate::ImError::invalid_input(
            Some("expected_profile_version".to_owned()),
            "avatar profile version exceeds the server range",
        ));
    }
    Ok(
        json!({"request_id": request_id.to_string(), "expected_profile_version": request.expected_profile_version}),
    )
}

pub(crate) fn build_set_avatar_rpc_call(
    request: crate::identity::SetAvatarRequest,
) -> crate::ImResult<super::RpcCall> {
    if request.image_jpeg.is_empty() || request.image_jpeg.len() > 512 * 1024 {
        return Err(crate::ImError::invalid_input(
            Some("image_jpeg".to_owned()),
            "avatar JPEG must be at most 512 KiB",
        ));
    }
    let mut params = avatar_params(crate::identity::ClearAvatarRequest {
        request_id: request.request_id,
        expected_profile_version: request.expected_profile_version,
    })?;
    params["image_base64"] =
        json!(base64::engine::general_purpose::STANDARD.encode(request.image_jpeg));
    Ok(super::rpc_call(
        super::DID_PROFILE_RPC_ENDPOINT,
        "set_avatar",
        super::TransportProfile::RpcDefault,
        params,
    ))
}

pub(crate) fn build_clear_avatar_rpc_call(
    request: crate::identity::ClearAvatarRequest,
) -> crate::ImResult<super::RpcCall> {
    Ok(super::rpc_call(
        super::DID_PROFILE_RPC_ENDPOINT,
        "clear_avatar",
        super::TransportProfile::RpcDefault,
        avatar_params(request)?,
    ))
}

pub(crate) fn build_profile_resolve_rpc_call(did: &str) -> crate::ImResult<super::RpcCall> {
    let did = super::required_trimmed(did, "did")?;
    Ok(super::rpc_call(
        super::DID_PROFILE_RPC_ENDPOINT,
        "resolve",
        super::TransportProfile::RpcDefault,
        json!({ "did": did }),
    ))
}

pub(crate) fn build_public_profile_rpc_call(did: &str) -> crate::ImResult<super::RpcCall> {
    let did = super::required_trimmed(did, "did")?;
    Ok(super::rpc_call(
        super::DID_PROFILE_RPC_ENDPOINT,
        "get_public_profile",
        super::TransportProfile::RpcReadHeavy,
        json!({ "did": did }),
    ))
}

pub(crate) fn build_get_me_profile_rpc_call() -> super::RpcCall {
    super::rpc_call(
        super::DID_PROFILE_RPC_ENDPOINT,
        "get_me",
        super::TransportProfile::RpcReadHeavy,
        json!({}),
    )
}

pub(crate) fn build_update_me_profile_rpc_call(
    params: super::UpdateProfileParams,
) -> crate::ImResult<super::ProfileUpdateCall> {
    let (payload, changed_fields) = build_update_profile_payload(params)?;
    Ok(super::ProfileUpdateCall {
        call: super::rpc_call(
            super::DID_PROFILE_RPC_ENDPOINT,
            "update_me",
            super::TransportProfile::RpcDefault,
            payload,
        ),
        changed_fields,
    })
}

pub(crate) fn build_update_profile_payload(
    params: super::UpdateProfileParams,
) -> crate::ImResult<(Value, Vec<String>)> {
    let mut payload = Map::new();
    let mut changed_fields = Vec::new();
    if !params.display_name.trim().is_empty() {
        payload.insert(
            "nick_name".to_string(),
            Value::String(params.display_name.trim().to_string()),
        );
        changed_fields.push("display_name".to_string());
    }
    if !params.bio.trim().is_empty() {
        payload.insert(
            "bio".to_string(),
            Value::String(params.bio.trim().to_string()),
        );
        changed_fields.push("bio".to_string());
    }
    if !params.tags_csv.trim().is_empty() {
        payload.insert(
            "tags".to_string(),
            json!(super::split_csv(&params.tags_csv)),
        );
        changed_fields.push("tags".to_string());
    }
    let avatar_url = if params.avatar_uri.trim().is_empty() {
        params.avatar_url.trim().to_string()
    } else {
        params.avatar_uri.trim().to_string()
    };
    if !avatar_url.trim().is_empty() {
        payload.insert(
            "avatar_url".to_string(),
            Value::String(avatar_url.trim().to_string()),
        );
        changed_fields.push("avatar_uri".to_string());
    }
    let markdown = if params.preserve_markdown {
        params.markdown.clone()
    } else {
        params.markdown.trim().to_string()
    };
    if !markdown.trim().is_empty() {
        payload.insert("profile_md".to_string(), Value::String(markdown));
        changed_fields.push("profile_md".to_string());
    }
    if payload.is_empty() {
        return Err(crate::ImError::invalid_input(
            None,
            "no profile fields were provided",
        ));
    }
    Ok((Value::Object(payload), changed_fields))
}
