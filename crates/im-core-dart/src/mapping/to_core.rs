use std::path::PathBuf;

use crate::dto::{
    auth::DartAuthScope,
    config::{DartImCoreConfig, DartImCorePaths, DartMessageTransportPolicy},
    directory::DartIdentitySubject,
    error::DartImError,
    group::DartCreateGroupRequest,
    identity::DartIdentitySelector,
    message::{DartMessageSecurityMode, DartMessageTarget, DartSendTextRequest, DartThreadRef},
    profile::DartProfilePatch,
};

impl TryFrom<DartImCoreConfig> for im_core::ImCoreConfig {
    type Error = DartImError;

    fn try_from(value: DartImCoreConfig) -> Result<Self, Self::Error> {
        let mut config = im_core::ImCoreConfig::new(
            im_core::ServiceEndpoint::parse(value.service_base_url).map_err(DartImError::from)?,
            value.did_domain,
        )
        .map_err(DartImError::from)?;
        config.user_service_endpoint = parse_endpoint(value.user_service_endpoint)?;
        config.message_service_endpoint = parse_endpoint(value.message_service_endpoint)?;
        config.anp_service_endpoint = parse_endpoint(value.anp_service_endpoint)?;
        config.anp_service_did = value
            .anp_service_did
            .map(im_core::ids::Did::parse)
            .transpose()
            .map_err(DartImError::from)?;
        config.transport_policy = value.transport_policy.into();
        Ok(config)
    }
}

fn parse_endpoint(value: Option<String>) -> Result<Option<im_core::ServiceEndpoint>, DartImError> {
    value
        .map(im_core::ServiceEndpoint::parse)
        .transpose()
        .map_err(DartImError::from)
}

impl From<DartMessageTransportPolicy> for im_core::MessageTransportPolicy {
    fn from(value: DartMessageTransportPolicy) -> Self {
        match value {
            DartMessageTransportPolicy::Auto => Self::Auto,
            DartMessageTransportPolicy::HttpOnly => Self::HttpOnly,
            DartMessageTransportPolicy::RealtimePreferred => Self::RealtimePreferred,
        }
    }
}

impl TryFrom<DartImCorePaths> for im_core::ImCorePaths {
    type Error = DartImError;

    fn try_from(value: DartImCorePaths) -> Result<Self, Self::Error> {
        Ok(Self {
            identities: im_core::IdentityRegistryPaths {
                identity_root_dir: PathBuf::from(value.identity_root_dir),
                registry_path: PathBuf::from(value.registry_path),
                default_identity_path: value.default_identity_path.map(PathBuf::from),
            },
            local_state: im_core::LocalStatePaths {
                sqlite_path: PathBuf::from(value.sqlite_path),
            },
            runtime: im_core::RuntimePaths {
                cache_dir: PathBuf::from(value.cache_dir),
                temp_dir: PathBuf::from(value.temp_dir),
            },
        })
    }
}

impl TryFrom<DartIdentitySelector> for im_core::identity::IdentitySelector {
    type Error = DartImError;

    fn try_from(value: DartIdentitySelector) -> Result<Self, Self::Error> {
        match value {
            DartIdentitySelector::Default => Ok(Self::Default),
            DartIdentitySelector::Id { id } => im_core::ids::IdentityId::parse(id)
                .map(Self::Id)
                .map_err(DartImError::from),
            DartIdentitySelector::Did { did } => im_core::ids::Did::parse(did)
                .map(Self::Did)
                .map_err(DartImError::from),
            DartIdentitySelector::Handle { handle } => im_core::ids::Handle::parse(handle, "")
                .map(Self::Handle)
                .map_err(DartImError::from),
            DartIdentitySelector::LocalAlias { alias } => Ok(Self::LocalAlias(alias)),
        }
    }
}

impl From<DartAuthScope> for im_core::auth::AuthScope {
    fn from(value: DartAuthScope) -> Self {
        match value {
            DartAuthScope::UserProfile => Self::UserProfile,
            DartAuthScope::Messaging => Self::Messaging,
            DartAuthScope::GroupMessaging => Self::GroupMessaging,
        }
    }
}

impl TryFrom<DartMessageTarget> for im_core::messages::MessageTarget {
    type Error = DartImError;

    fn try_from(value: DartMessageTarget) -> Result<Self, Self::Error> {
        match value {
            DartMessageTarget::Direct { peer } => im_core::ids::PeerRef::parse(peer, "")
                .map(Self::Direct)
                .map_err(DartImError::from),
            DartMessageTarget::Group { group } => im_core::ids::GroupRef::parse(group)
                .map(Self::Group)
                .map_err(DartImError::from),
        }
    }
}

impl TryFrom<DartThreadRef> for im_core::messages::ThreadRef {
    type Error = DartImError;

    fn try_from(value: DartThreadRef) -> Result<Self, Self::Error> {
        match value {
            DartThreadRef::Direct { peer } => im_core::ids::PeerRef::parse(peer, "")
                .map(Self::Direct)
                .map_err(DartImError::from),
            DartThreadRef::Group { group } => im_core::ids::GroupRef::parse(group)
                .map(Self::Group)
                .map_err(DartImError::from),
            DartThreadRef::Thread { thread_id } => im_core::ids::ThreadId::parse(thread_id)
                .map(Self::Thread)
                .map_err(DartImError::from),
        }
    }
}

impl From<DartMessageSecurityMode> for im_core::messages::MessageSecurityMode {
    fn from(value: DartMessageSecurityMode) -> Self {
        match value {
            DartMessageSecurityMode::DefaultPlain => Self::DefaultPlain,
            DartMessageSecurityMode::Plain => Self::Plain,
            DartMessageSecurityMode::SecureDirect => Self::SecureDirect,
            DartMessageSecurityMode::GroupE2ee => Self::GroupE2ee,
        }
    }
}

impl TryFrom<DartSendTextRequest> for im_core::messages::SendMessageRequest {
    type Error = DartImError;

    fn try_from(value: DartSendTextRequest) -> Result<Self, Self::Error> {
        Ok(Self {
            target: value.target.try_into()?,
            body: im_core::messages::MessageBody::Text {
                text: value.text,
                kind: if value.markdown {
                    im_core::messages::MessageKind::Markdown
                } else {
                    im_core::messages::MessageKind::Text
                },
            },
            security: value.security.into(),
            client_message_id: value
                .client_message_id
                .map(im_core::ids::MessageId::parse)
                .transpose()
                .map_err(DartImError::from)?,
            delivery: im_core::messages::MessageDeliveryOptions {
                idempotency_key: value.idempotency_key,
                wait_for_final_acceptance: value.wait_for_final_acceptance,
            },
        })
    }
}

impl DartCreateGroupRequest {
    pub fn into_core(
        self,
        default_service_did: Option<String>,
    ) -> Result<im_core::groups::GroupCreateRequest, DartImError> {
        let service_did = self.service_did.or(default_service_did).ok_or_else(|| {
            DartImError::invalid_input(
                Some("service_did".to_string()),
                "group create requires service_did or ImCoreConfig.anp_service_did",
            )
        })?;
        Ok(im_core::groups::GroupCreateRequest {
            name: self.name,
            description: self.description,
            discoverability: self.discoverability,
            admission_mode: self.admission_mode,
            message_security_profile: self.message_security_profile,
            e2ee: self.e2ee,
            slug: self.slug,
            goal: self.goal,
            rules: self.rules,
            message_prompt: self.message_prompt,
            doc_url: self.doc_url,
            attachments_allowed: self.attachments_allowed,
            max_members: self.max_members,
            member_max_messages: self.member_max_messages,
            member_max_total_chars: self.member_max_total_chars,
            service_did: im_core::ids::Did::parse(service_did).map_err(DartImError::from)?,
        })
    }
}

impl From<DartProfilePatch> for im_core::identity::ProfilePatch {
    fn from(value: DartProfilePatch) -> Self {
        Self {
            display_name: value.display_name,
            bio: value.bio,
            tags: value.tags,
            markdown: value.markdown,
        }
    }
}

impl TryFrom<DartIdentitySubject> for im_core::directory::IdentitySubject {
    type Error = DartImError;

    fn try_from(value: DartIdentitySubject) -> Result<Self, Self::Error> {
        match value {
            DartIdentitySubject::Did { did } => im_core::ids::Did::parse(did)
                .map(Self::Did)
                .map_err(DartImError::from),
            DartIdentitySubject::Handle { handle } => im_core::ids::Handle::parse(handle, "")
                .map(Self::Handle)
                .map_err(DartImError::from),
            DartIdentitySubject::Any { value } => Ok(Self::Any(value)),
        }
    }
}
