use serde_json::Value;

use crate::internal::auth::session::SessionProvider;
use crate::internal::transport::AuthenticatedRpcTransport;

pub(crate) struct ProfileReader<'a, P, T> {
    client: &'a crate::core::ImClient,
    session_provider: P,
    transport: T,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ProfileReadResult {
    pub(crate) profile: crate::identity::Profile,
    pub(crate) raw: Value,
}

impl<'a, P, T> ProfileReader<'a, P, T>
where
    P: SessionProvider,
    T: AuthenticatedRpcTransport,
{
    pub(crate) fn new(
        client: &'a crate::core::ImClient,
        session_provider: P,
        transport: T,
    ) -> Self {
        Self {
            client,
            session_provider,
            transport,
        }
    }

    pub(crate) fn profile(mut self) -> crate::ImResult<ProfileReadResult> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::UserProfile)?;
        let call = crate::internal::identity_wire::profile::build_get_me_profile_rpc_call();
        let raw = self
            .transport
            .authenticated_rpc(call.endpoint, call.method, call.params)?;
        let profile = profile_from_value(self.client, &raw)?;
        Ok(ProfileReadResult { profile, raw })
    }
}

pub(crate) fn profile_from_value(
    client: &crate::core::ImClient,
    raw: &Value,
) -> crate::ImResult<crate::identity::Profile> {
    let subject = string_value(raw, &["did", "subject", "id"])
        .filter(|value| value.starts_with("did:"))
        .map(crate::ids::Did::parse)
        .transpose()?
        .unwrap_or_else(|| client.did().clone());
    let handle = string_value(raw, &["handle", "full_handle"])
        .filter(|value| !value.trim().is_empty())
        .map(|value| crate::ids::Handle::parse(&value, ""))
        .transpose()?
        .or_else(|| client.handle().cloned());
    let display_name = string_value(raw, &["display_name", "nick_name", "name"]);
    let bio = string_value(raw, &["bio"]);
    let tags = tags_value(raw.get("tags"));
    let markdown = string_value(raw, &["markdown", "profile_md"]);
    let avatar_url = string_value(raw, &["avatar_url", "avatar"]);
    let updated_at = string_value(raw, &["updated_at", "update_time", "updatedAt"]);
    let metadata = metadata_value(raw.get("metadata"));

    Ok(crate::identity::Profile {
        subject,
        handle,
        display_name,
        bio,
        tags,
        markdown,
        avatar_url,
        updated_at,
        metadata,
    })
}

fn string_value(raw: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|key| raw.get(*key))
        .find_map(|value| match value {
            Value::String(value) if !value.trim().is_empty() => Some(value.clone()),
            _ => None,
        })
}

fn tags_value(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
        Some(Value::String(value)) => value
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

fn metadata_value(value: Option<&Value>) -> Vec<crate::identity::ProfileAttribute> {
    match value {
        Some(Value::Object(object)) => object
            .iter()
            .filter_map(|(key, value)| {
                let value = match value {
                    Value::String(value) => value.clone(),
                    Value::Null => return None,
                    value => value.to_string(),
                };
                Some(crate::identity::ProfileAttribute {
                    key: key.clone(),
                    value,
                })
            })
            .collect(),
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(|value| {
                let object = value.as_object()?;
                let key = object.get("key").and_then(Value::as_str)?;
                let value = object.get("value")?;
                let value = match value {
                    Value::String(value) => value.clone(),
                    Value::Null => return None,
                    value => value.to_string(),
                };
                Some(crate::identity::ProfileAttribute {
                    key: key.to_string(),
                    value,
                })
            })
            .collect(),
        _ => Vec::new(),
    }
}
