use im_core::prelude::{
    AuthScope, Did, Handle, IdentitySelector, InitialProfile, PeerRef, ProfilePatch,
    RegisterHandleRequest, SessionBundle, VerificationInput,
};
use serde_json::Value;

use crate::cli::ParsedCommand;
use crate::identity;
use crate::output::ExitError;
use crate::transportcfg::Profile;

#[derive(Debug, Clone)]
pub struct RegisterHandleBridgeRequest {
    pub sdk: RegisterHandleRequest,
    pub legacy: identity::RegisterParams,
}

#[derive(Debug, Clone)]
pub struct GetProfileBridgeRequest {
    pub self_profile: bool,
    pub handle: String,
    pub did: String,
}

#[derive(Debug, Clone)]
pub struct ResolveBridgeRequest {
    pub handle: String,
    pub did: String,
}

#[derive(Debug, Clone)]
pub struct SetProfileBridgeRequest {
    pub patch: ProfilePatch,
    pub legacy: identity::SetProfileParams,
}

pub fn cli_identity_selector(identity_flag: &str) -> IdentitySelector {
    let value = identity_flag.trim();
    if value.is_empty() || value == "default" {
        return IdentitySelector::Default;
    }
    if value.starts_with("did:") {
        return Did::parse(value)
            .map(IdentitySelector::Did)
            .unwrap_or_else(|_| IdentitySelector::LocalAlias(value.to_string()));
    }
    if looks_like_handle(value) {
        return Handle::parse(value, "")
            .map(IdentitySelector::Handle)
            .unwrap_or_else(|_| IdentitySelector::LocalAlias(value.to_string()));
    }
    IdentitySelector::LocalAlias(value.to_string())
}

pub fn register_handle_request(
    command: &ParsedCommand,
) -> Result<RegisterHandleRequest, ExitError> {
    let handle = string_flag(command, "handle");
    let requested_handle = Handle::parse(&handle, "").map_err(|err| {
        ExitError::new(
            "invalid_argument",
            2,
            format!("invalid --handle: {err}"),
            "Use a non-empty handle local part or full handle.",
        )
    })?;
    let local_alias = trimmed_optional(&command.globals.identity);
    let otp = string_flag(command, "otp");
    Ok(RegisterHandleRequest {
        local_alias,
        requested_handle,
        verification: if otp.trim().is_empty() {
            VerificationInput::AlreadyVerified
        } else {
            VerificationInput::Otp {
                code: otp.trim().to_string(),
            }
        },
        profile: InitialProfile {
            display_name: trimmed_optional(&string_flag(command, "display-name")),
            avatar_url: trimmed_optional(&string_flag(command, "avatar-url")),
        },
        make_default: !command
            .flags
            .get("no-default")
            .is_some_and(|value| value == "true"),
    })
}

pub fn register_handle_bridge_request(
    command: &ParsedCommand,
    identity_flag: &str,
) -> Result<RegisterHandleBridgeRequest, ExitError> {
    let mut sdk_command = command.clone();
    sdk_command.globals.identity = identity_flag.to_string();
    let sdk = register_handle_request(&sdk_command)?;
    let legacy = identity::RegisterParams {
        identity_name: identity_flag.to_string(),
        handle: string_flag(command, "handle"),
        phone: string_flag(command, "phone"),
        email: string_flag(command, "email"),
        otp: string_flag(command, "otp"),
        invite_code: string_flag(command, "invite-code"),
        wait: command
            .flags
            .get("wait")
            .is_some_and(|value| value == "true"),
        verification_timeout: 300,
        poll_interval_seconds: 5.0,
    };
    Ok(RegisterHandleBridgeRequest { sdk, legacy })
}

pub fn register_handle_plan_via_im_core(
    manager: &identity::Manager,
    did_domain: &str,
    command: &ParsedCommand,
    identity_flag: &str,
) -> Result<identity::CommandResult, ExitError> {
    let bridge = register_handle_bridge_request(command, identity_flag)?;
    let _sdk_request = bridge.sdk;
    identity::register_plan(manager, did_domain, &bridge.legacy).map_err(crate::app::identity_exit)
}

pub fn register_handle_via_im_core(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
    command: &ParsedCommand,
    identity_flag: &str,
) -> Result<identity::CommandResult, ExitError> {
    let bridge = register_handle_bridge_request(command, identity_flag)?;
    let core = super::build_im_core(resolved, manager)?;
    core.identities()
        .register_handle(bridge.sdk)
        .map_err(|err| super::map_im_error(err, "id register"))?;
    identity::register(resolved, manager, bridge.legacy).map_err(crate::app::identity_exit)
}

pub fn get_profile_request(command: &ParsedCommand) -> GetProfileBridgeRequest {
    GetProfileBridgeRequest {
        self_profile: command
            .flags
            .get("self")
            .is_some_and(|value| value == "true"),
        handle: string_flag(command, "handle"),
        did: string_flag(command, "did"),
    }
}

pub fn get_self_profile_via_im_core(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
    identity_flag: &str,
) -> Result<identity::CommandResult, ExitError> {
    let selector = cli_identity_selector(identity_flag);
    let client = super::build_im_client(resolved, manager, selector)?;
    let record = identity::service::load_identity_for_mutation(resolved, manager, identity_flag)
        .map_err(crate::app::identity_exit)?;
    let result = im_core::compat::profile::read_self_profile_with_bridge(
        &client,
        ProfileSessionProvider {
            subject: client.did().clone(),
            resolved,
            manager,
            record: record.clone(),
        },
        ProfileLegacyTransport {
            resolved,
            manager,
            record,
        },
    )
    .map_err(|err| super::map_im_error(err, "id profile get"))?;
    Ok(identity::wire::profile_self_result(result.raw))
}

pub fn get_public_profile_via_im_core(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
    identity_flag: &str,
    request: GetProfileBridgeRequest,
) -> Result<identity::CommandResult, ExitError> {
    let Some(client) =
        build_optional_directory_client(resolved, manager, identity_flag, "id profile get")?
    else {
        return identity::get_profile(
            resolved,
            manager,
            identity::GetProfileParams {
                self_profile: request.self_profile,
                handle: request.handle,
                did: request.did,
            },
        )
        .map_err(crate::app::identity_exit);
    };
    let mut subject = serde_json::Map::new();
    let profile_did = request.did.trim().to_string();
    if !request.handle.trim().is_empty() {
        let target = identity::normalize_handle_input(&request.handle, &resolved.did_domain)
            .map_err(crate::app::identity_exit)?;
        let peer = PeerRef::parse(&target.full_handle, "")
            .map_err(|err| super::map_im_error(err, "id profile get"))?;
        let result = im_core::compat::directory::resolve_peer_with_bridge(
            &client,
            peer,
            DirectoryLegacyTransport { resolved },
        )
        .map_err(|err| super::map_im_error(err, "id profile get"))?;
        let did = result.resolution.did.as_str().to_string();
        subject.insert("handle".to_string(), Value::String(target.local_part));
        subject.insert("full_handle".to_string(), Value::String(target.full_handle));
        subject.insert("domain".to_string(), Value::String(target.effective_domain));
        subject.insert("did".to_string(), Value::String(did));
        let profile = result.public_profile.unwrap_or(Value::Null);
        return Ok(identity::wire::profile_public_result(
            Value::Object(subject),
            profile,
        ));
    }
    if !profile_did.trim().is_empty() {
        subject.insert("did".to_string(), Value::String(profile_did.clone()));
    }
    let peer = PeerRef::parse(&profile_did, "")
        .map_err(|err| super::map_im_error(err, "id profile get"))?;
    let result = im_core::compat::directory::resolve_peer_with_bridge(
        &client,
        peer,
        DirectoryLegacyTransport { resolved },
    )
    .map_err(|err| super::map_im_error(err, "id profile get"))?;
    Ok(identity::wire::profile_public_result(
        Value::Object(subject),
        result.public_profile.unwrap_or(Value::Null),
    ))
}

pub fn resolve_request(command: &ParsedCommand) -> ResolveBridgeRequest {
    ResolveBridgeRequest {
        handle: string_flag(command, "handle"),
        did: string_flag(command, "did"),
    }
}

pub fn resolve_identity_via_im_core(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
    identity_flag: &str,
    request: ResolveBridgeRequest,
) -> Result<identity::CommandResult, ExitError> {
    let handle = request.handle.trim();
    let did = request.did.trim();
    if (handle.is_empty() && did.is_empty()) || (!handle.is_empty() && !did.is_empty()) {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            "invalid input: exactly one of handle or did is required",
            "Pass either --handle <handle> or --did <did>.",
        ));
    }
    let Some(client) =
        build_optional_directory_client(resolved, manager, identity_flag, "id resolve")?
    else {
        return identity::resolve_identity(
            resolved,
            identity::ResolveParams {
                handle: request.handle,
                did: request.did,
            },
        )
        .map_err(crate::app::identity_exit);
    };
    let peer = if !handle.is_empty() {
        let target = identity::normalize_handle_input(handle, &resolved.did_domain)
            .map_err(crate::app::identity_exit)?;
        PeerRef::parse(&target.full_handle, "")
            .map_err(|err| super::map_im_error(err, "id resolve"))?
    } else {
        PeerRef::parse(did, "").map_err(|err| super::map_im_error(err, "id resolve"))?
    };
    let result = im_core::compat::directory::resolve_peer_with_bridge(
        &client,
        peer,
        DirectoryLegacyTransport { resolved },
    )
    .map_err(|err| super::map_im_error(err, "id resolve"))?;
    Ok(identity::wire::resolve_result(
        result.resolve,
        result.lookup,
        result.public_profile,
        result.resolution.warnings,
    ))
}

fn build_optional_directory_client(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
    identity_flag: &str,
    context: &'static str,
) -> Result<Option<im_core::ImClient>, ExitError> {
    let core = super::build_im_core(resolved, manager)?;
    match core.client(cli_identity_selector(identity_flag)) {
        Ok(client) => Ok(Some(client)),
        Err(im_core::ImError::DefaultIdentityMissing)
        | Err(im_core::ImError::IdentityRequired)
        | Err(im_core::ImError::IdentityNotFound { .. }) => Ok(None),
        Err(err) => Err(super::map_im_error(err, context)),
    }
}

pub fn set_profile_request(
    display_name: String,
    bio: String,
    tags_csv: String,
    markdown: String,
    markdown_file: String,
) -> Result<SetProfileBridgeRequest, ExitError> {
    let legacy = identity::SetProfileParams {
        display_name,
        bio,
        tags_csv,
        markdown,
        markdown_file,
    };
    let patch = profile_patch_from_legacy_params(&legacy)?;
    Ok(SetProfileBridgeRequest { patch, legacy })
}

pub fn set_profile_via_im_core(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
    identity_flag: &str,
    request: SetProfileBridgeRequest,
) -> Result<identity::CommandResult, ExitError> {
    let selector = cli_identity_selector(identity_flag);
    let client = super::build_im_client(resolved, manager, selector)?;
    let record = identity::service::load_identity_for_mutation(resolved, manager, identity_flag)
        .map_err(crate::app::identity_exit)?;
    let identity = identity::store::identity_summary_from_record(&record);
    let result = im_core::compat::profile::update_profile_with_bridge(
        &client,
        request.patch,
        ProfileSessionProvider {
            subject: client.did().clone(),
            resolved,
            manager,
            record: record.clone(),
        },
        ProfileLegacyTransport {
            resolved,
            manager,
            record: record.clone(),
        },
    )
    .map_err(|err| super::map_im_error(err, "id profile set"))?;
    let display_name = request.legacy.display_name.trim();
    if !display_name.is_empty() {
        let _ = manager.update_display_name(&record.identity_name, display_name);
    }
    Ok(identity::wire::profile_update_result(
        &identity,
        result.changed_fields,
        result.raw,
    ))
}

fn profile_patch_from_legacy_params(
    params: &identity::SetProfileParams,
) -> Result<ProfilePatch, ExitError> {
    let markdown_file = params.markdown_file.trim();
    let markdown = if markdown_file.is_empty() {
        trimmed_optional(&params.markdown)
    } else {
        let raw = std::fs::read(&params.markdown_file).map_err(|err| {
            ExitError::new(
                "invalid_argument",
                2,
                format!("read markdown file {markdown_file:?}: {err}"),
                "Check the --markdown-file path and permissions.",
            )
        })?;
        let markdown = String::from_utf8_lossy(&raw).into_owned();
        (!markdown.trim().is_empty()).then_some(markdown)
    };
    Ok(ProfilePatch {
        display_name: trimmed_optional(&params.display_name),
        bio: trimmed_optional(&params.bio),
        tags: tags_patch(&params.tags_csv),
        markdown,
    })
}

fn tags_patch(tags_csv: &str) -> Option<Vec<String>> {
    let tags = tags_csv
        .split(',')
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    (!tags.is_empty()).then_some(tags)
}

struct ProfileSessionProvider<'a> {
    subject: Did,
    resolved: &'a crate::config::Resolved,
    manager: &'a identity::Manager,
    record: identity::types::StoredIdentity,
}

impl im_core::compat::profile::BridgeProfileSessionProvider for ProfileSessionProvider<'_> {
    fn ensure_profile_session(&self) -> im_core::ImResult<SessionBundle> {
        let session = identity::service::auth_session(self.resolved, self.manager, &self.record)
            .map_err(identity_error_to_im_error)?;
        Ok(SessionBundle {
            subject: self.subject.clone(),
            scope: AuthScope::UserProfile,
            expires_at: None,
            refreshed: session.current_jwt().trim() != self.record.jwt_token.trim(),
        })
    }
}

struct ProfileLegacyTransport<'a> {
    resolved: &'a crate::config::Resolved,
    manager: &'a identity::Manager,
    record: identity::types::StoredIdentity,
}

struct DirectoryLegacyTransport<'a> {
    resolved: &'a crate::config::Resolved,
}

impl im_core::compat::directory::BridgeDirectoryRpcTransport for DirectoryLegacyTransport<'_> {
    fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> im_core::ImResult<Value> {
        let client =
            identity::client::Client::new(self.resolved).map_err(identity_error_to_im_error)?;
        let profile = match method {
            "get_public_profile" => Profile::RpcReadHeavy,
            _ => Profile::RpcDefault,
        };
        client
            .rpc_call_profile(profile, endpoint, method, params)
            .map_err(identity_error_to_im_error)
    }
}

impl im_core::compat::profile::BridgeProfileAuthenticatedRpcTransport
    for ProfileLegacyTransport<'_>
{
    fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> im_core::ImResult<Value> {
        read_authenticated_profile_with_fallback(
            self.resolved,
            self.manager,
            &self.record,
            endpoint,
            method,
            params,
        )
        .map_err(identity_error_to_im_error)
    }
}

fn read_authenticated_profile_with_fallback(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
    record: &identity::types::StoredIdentity,
    endpoint: &str,
    method: &str,
    params: Value,
) -> Result<Value, identity::IdentityError> {
    match read_authenticated_profile(resolved, manager, record, endpoint, method, params.clone()) {
        Ok(result) => Ok(result),
        Err(err) if identity_error_is_unauthorized(&err) => {
            let refreshed = identity::refresh_token(resolved, manager, &record.identity_name).ok();
            let record = refreshed
                .as_ref()
                .and_then(|_| manager.load(&record.identity_name).ok())
                .unwrap_or_else(|| record.clone());
            match read_authenticated_profile(resolved, manager, &record, endpoint, method, params) {
                Ok(result) => Ok(result),
                Err(_) => Err(err),
            }
        }
        Err(err) => Err(err),
    }
}

fn read_authenticated_profile(
    resolved: &crate::config::Resolved,
    manager: &identity::Manager,
    record: &identity::types::StoredIdentity,
    endpoint: &str,
    method: &str,
    params: Value,
) -> Result<Value, identity::IdentityError> {
    let mut auth = identity::service::auth_session(resolved, manager, record)?;
    let client = identity::client::Client::new(resolved)?;
    let profile = match method {
        "get_me" => Profile::RpcReadHeavy,
        _ => Profile::RpcDefault,
    };
    client.authenticated_rpc_call_profile(profile, endpoint, method, params, &mut auth)
}

fn identity_error_is_unauthorized(err: &identity::IdentityError) -> bool {
    matches!(
        err,
        identity::IdentityError::Service(service)
            if service.status_code == 401 || service.rpc_code == -32001
    )
}

fn identity_error_to_im_error(err: identity::IdentityError) -> im_core::ImError {
    match err {
        identity::IdentityError::InvalidInput(message) => {
            im_core::ImError::invalid_input(None, message)
        }
        identity::IdentityError::NotFound(message)
        | identity::IdentityError::NoDefaultIdentity(message) => {
            im_core::ImError::IdentityNotFound { selector: message }
        }
        identity::IdentityError::AuthRequired(_) => im_core::ImError::AuthRequired,
        identity::IdentityError::Service(service) => im_core::ImError::Service {
            status_code: (service.status_code != 0).then_some(service.status_code),
            code: (service.rpc_code != 0).then(|| service.rpc_code.to_string()),
            message: service.message,
        },
        identity::IdentityError::Io(err) => im_core::ImError::Io {
            detail: err.to_string(),
        },
        identity::IdentityError::Json(err) => im_core::ImError::Serialization {
            detail: err.to_string(),
        },
        err => im_core::ImError::Internal {
            message: err.to_string(),
        },
    }
}

fn looks_like_handle(value: &str) -> bool {
    value.starts_with('@') || value.contains('.')
}

fn string_flag(command: &ParsedCommand, name: &str) -> String {
    command.flags.get(name).cloned().unwrap_or_default()
}

fn trimmed_optional(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
