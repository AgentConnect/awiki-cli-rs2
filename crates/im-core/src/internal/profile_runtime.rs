use serde_json::Value;

use crate::internal::auth::session::{AsyncSessionProvider, SessionProvider};
use crate::internal::transport::{AsyncAuthenticatedRpcTransport, AuthenticatedRpcTransport};

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

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ProfileUpdateResult {
    pub(crate) profile: crate::identity::Profile,
    pub(crate) raw: Value,
    pub(crate) changed_fields: Vec<String>,
}

impl<'a, P, T> ProfileReader<'a, P, T> {
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
}

impl<'a, P, T> ProfileReader<'a, P, T>
where
    P: SessionProvider,
    T: AuthenticatedRpcTransport,
{
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

    pub(crate) fn update_profile(
        mut self,
        patch: crate::identity::ProfilePatch,
    ) -> crate::ImResult<ProfileUpdateResult> {
        let params = update_profile_params_from_patch(patch);
        let requested_display_name = params.display_name.trim().to_string();
        let update_call =
            crate::internal::identity_wire::profile::build_update_me_profile_rpc_call(params)?;
        self.session_provider
            .ensure_session(crate::auth::AuthScope::UserProfile)?;
        let raw = self.transport.authenticated_rpc(
            update_call.call.endpoint,
            update_call.call.method,
            update_call.call.params,
        )?;
        let profile = profile_from_value(self.client, &raw)?;
        if update_call
            .changed_fields
            .iter()
            .any(|field| field == "display_name")
        {
            let display_name = profile
                .display_name
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(requested_display_name.as_str());
            if !display_name.is_empty() {
                let _ = crate::internal::identity_store::IdentityStore::new(
                    &self.client.core_inner().sdk_paths().identities,
                )
                .update_display_name_projection(self.client.current_identity(), display_name);
            }
        }
        Ok(ProfileUpdateResult {
            profile,
            raw,
            changed_fields: update_call.changed_fields,
        })
    }
}

impl<'a, P, T> ProfileReader<'a, P, T>
where
    P: AsyncSessionProvider,
    T: AsyncAuthenticatedRpcTransport,
{
    pub(crate) async fn profile_async(mut self) -> crate::ImResult<ProfileReadResult> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::UserProfile)
            .await?;
        let call = crate::internal::identity_wire::profile::build_get_me_profile_rpc_call();
        let raw = self
            .transport
            .authenticated_rpc(call.endpoint, call.method, call.params)
            .await?;
        let profile = profile_from_value(self.client, &raw)?;
        Ok(ProfileReadResult { profile, raw })
    }

    pub(crate) async fn update_profile_async(
        mut self,
        patch: crate::identity::ProfilePatch,
    ) -> crate::ImResult<ProfileUpdateResult> {
        let params = update_profile_params_from_patch(patch);
        let requested_display_name = params.display_name.trim().to_string();
        let update_call =
            crate::internal::identity_wire::profile::build_update_me_profile_rpc_call(params)?;
        self.session_provider
            .ensure_session(crate::auth::AuthScope::UserProfile)
            .await?;
        let raw = self
            .transport
            .authenticated_rpc(
                update_call.call.endpoint,
                update_call.call.method,
                update_call.call.params,
            )
            .await?;
        let profile = profile_from_value(self.client, &raw)?;
        if update_call
            .changed_fields
            .iter()
            .any(|field| field == "display_name")
        {
            let display_name = profile
                .display_name
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(requested_display_name.as_str());
            if !display_name.is_empty() {
                let paths = self.client.core_inner().sdk_paths().identities.clone();
                let identity = self.client.current_identity().clone();
                let display_name = display_name.to_string();
                let _ = crate::internal::runtime::worker::run_blocking(move || {
                    crate::internal::identity_store::IdentityStore::new(&paths)
                        .update_display_name_projection(&identity, &display_name)
                })
                .await;
            }
        }
        Ok(ProfileUpdateResult {
            profile,
            raw,
            changed_fields: update_call.changed_fields,
        })
    }
}

pub(crate) fn update_profile_params_from_patch(
    patch: crate::identity::ProfilePatch,
) -> crate::internal::identity_wire::UpdateProfileParams {
    crate::internal::identity_wire::UpdateProfileParams {
        display_name: patch.display_name.unwrap_or_default(),
        bio: patch.bio.unwrap_or_default(),
        tags_csv: patch.tags.map(|tags| tags.join(",")).unwrap_or_default(),
        markdown: patch.markdown.unwrap_or_default(),
        avatar_uri: patch.avatar_uri.unwrap_or_default(),
        avatar_url: patch.avatar_url.unwrap_or_default(),
        preserve_markdown: true,
    }
}

pub(crate) fn profile_from_value(
    client: &crate::core::ImClient,
    raw: &Value,
) -> crate::ImResult<crate::identity::Profile> {
    let subject = string_value(raw, &["subject_did", "did", "subject", "id"])
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
    let description = string_value(raw, &["description", "bio"]);
    let bio = string_value(raw, &["bio", "description"]);
    let tags = tags_value(raw.get("tags"));
    let markdown = string_value(raw, &["markdown", "profile_md"]);
    let avatar_uri = string_value(raw, &["avatar_uri", "avatar_url", "avatar"]);
    let avatar_url = string_value(raw, &["avatar_url", "avatar", "avatar_uri"]);
    let profile_uri = string_value(raw, &["profile_uri", "profile_url"]);
    let subject_type = string_value(raw, &["subject_type"]);
    let updated_at = string_value(raw, &["updated", "updated_at", "update_time", "updatedAt"]);
    let version_id = string_value(raw, &["versionId", "version_id"]);
    let ttl = u64_value(raw.get("ttl"));
    let proof = raw.get("proof").cloned();
    let metadata = profile_metadata(raw);

    Ok(crate::identity::Profile {
        subject,
        handle,
        display_name,
        bio,
        description,
        tags,
        markdown,
        avatar_uri,
        avatar_url,
        profile_uri,
        subject_type,
        updated_at,
        version_id,
        ttl,
        proof,
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

fn u64_value(value: Option<&Value>) -> Option<u64> {
    match value {
        Some(Value::Number(value)) => value.as_u64(),
        Some(Value::String(value)) => value.trim().parse().ok(),
        _ => None,
    }
}

fn profile_metadata(raw: &Value) -> Vec<crate::identity::ProfileAttribute> {
    let mut metadata = metadata_value(raw.get("metadata"));
    for key in [
        "type",
        "avatar_uri",
        "avatar_url",
        "profile_uri",
        "profile_url",
        "description",
        "subject_type",
        "discoverability",
        "updated",
        "versionId",
        "version_id",
        "ttl",
    ] {
        let Some(value) = raw.get(key) else {
            continue;
        };
        if metadata.iter().any(|attribute| attribute.key == key) {
            continue;
        }
        let value = match value {
            Value::String(value) => value.clone(),
            Value::Null => continue,
            value => value.to_string(),
        };
        metadata.push(crate::identity::ProfileAttribute {
            key: key.to_string(),
            value,
        });
    }
    metadata
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
