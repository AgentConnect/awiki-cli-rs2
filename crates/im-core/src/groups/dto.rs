use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupReadResult {
    pub group: Option<GroupSnapshot>,
    pub groups: Vec<GroupSummary>,
    pub members: Vec<GroupMember>,
    #[serde(default)]
    pub resolved_member: Option<GroupMemberResolution>,
    pub messages: crate::ids::Page<crate::messages::Message>,
    pub total: Option<u32>,
    pub source: Option<String>,
    #[serde(skip)]
    raw_response: Option<Value>,
    pub warnings: Vec<String>,
}

impl GroupReadResult {
    pub(crate) fn from_raw_response(raw: Value, warnings: Vec<String>) -> Self {
        let warnings = merge_raw_warnings(raw.get("warnings"), warnings);
        let group = group_snapshot_from_response(&raw);
        let groups = values_from_array(raw.get("groups"))
            .into_iter()
            .filter_map(group_summary_from_value)
            .collect();
        let members = values_from_array(raw.get("members"))
            .into_iter()
            .filter_map(group_member_from_value)
            .collect();
        let message_items = values_from_array(raw.get("messages"))
            .into_iter()
            .filter_map(group_message_from_value)
            .collect::<Vec<_>>();
        let messages = crate::ids::Page {
            items: message_items,
            next_cursor: cursor_from_value(
                raw.get("next_cursor").or_else(|| raw.get("next_since_seq")),
            ),
            has_more: bool_value(raw.get("has_more")),
        };
        Self {
            group,
            groups,
            members,
            resolved_member: None,
            messages,
            total: u32_value(raw.get("total")),
            source: optional_string(raw.get("source")),
            raw_response: Some(raw),
            warnings,
        }
    }

    pub fn response_json(&self) -> Option<&Value> {
        self.raw_response.as_ref()
    }

    pub(crate) fn raw_response(&self) -> Option<&Value> {
        self.raw_response.as_ref()
    }

    pub(crate) fn merge_group_snapshot_from(&mut self, other: &Self) {
        if other.group.is_some() {
            self.group = other.group.clone();
        }
        self.warnings.extend(other.warnings.iter().cloned());
    }

    pub(crate) fn merge_group_members_from(&mut self, other: &Self) {
        self.members = other.members.clone();
        if other.total.is_some() {
            self.total = other.total;
        }
        self.warnings.extend(other.warnings.iter().cloned());
    }

    pub(crate) fn push_warning(&mut self, warning: impl Into<String>) {
        self.warnings.push(warning.into());
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupCreateRequest {
    pub name: String,
    pub description: Option<String>,
    pub avatar_uri: Option<String>,
    pub discoverability: Option<GroupDiscoverability>,
    pub admission_mode: Option<GroupAdmissionMode>,
    pub message_security_profile: Option<GroupMessageSecurityProfile>,
    #[serde(default)]
    pub security: GroupSecurityRequirement,
    pub e2ee: bool,
    pub slug: Option<String>,
    pub goal: Option<String>,
    pub rules: Option<String>,
    pub message_prompt: Option<String>,
    pub doc_url: Option<String>,
    pub attachments_allowed: Option<bool>,
    pub max_members: Option<GroupMemberLimit>,
    pub member_max_messages: Option<i64>,
    pub member_max_total_chars: Option<i64>,
}

impl GroupCreateRequest {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            avatar_uri: None,
            discoverability: None,
            admission_mode: None,
            message_security_profile: None,
            security: GroupSecurityRequirement::default(),
            e2ee: false,
            slug: None,
            goal: None,
            rules: None,
            message_prompt: None,
            doc_url: None,
            attachments_allowed: None,
            max_members: None,
            member_max_messages: None,
            member_max_total_chars: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupJoinRequest {
    pub group: crate::ids::GroupRef,
    pub reason_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupLeaveRequest {
    pub group: crate::ids::GroupRef,
    pub reason_text: Option<String>,
    #[serde(default)]
    pub security: GroupSecurityRequirement,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupMemberMutationRequest {
    pub group: crate::ids::GroupRef,
    pub member: GroupMemberRef,
    pub role: Option<GroupMemberRole>,
    pub reason_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leave_request_id: Option<String>,
    #[serde(default)]
    pub security: GroupSecurityRequirement,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupKeyPackagePublishRequest {
    pub purpose: GroupKeyPackagePurpose,
    pub group: Option<crate::ids::GroupRef>,
    pub device_id: Option<String>,
    pub key_package_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupKeyPackagePublishResult {
    pub owner_did: crate::ids::Did,
    pub device_id: String,
    pub key_package_id: String,
    pub purpose: GroupKeyPackagePurpose,
    pub group: Option<crate::ids::GroupRef>,
    pub raw_response: Value,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupE2eeProcessLeaveRequest {
    pub group: crate::ids::GroupRef,
    pub member: GroupMemberRef,
    pub leave_request_id: String,
    pub reason_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupE2eeUpdateKeyRequest {
    pub group: crate::ids::GroupRef,
    pub member: GroupMemberRef,
    pub device_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupE2eeRecoverMemberRequest {
    pub group: crate::ids::GroupRef,
    pub member: GroupMemberRef,
    pub device_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupKeyPackagePurpose {
    Normal,
    Recovery,
    Update,
    Custom(String),
}

impl GroupKeyPackagePurpose {
    pub fn parse(input: impl Into<String>) -> crate::ImResult<Self> {
        parse_group_token(input, "purpose", |value| match value {
            "normal" => Self::Normal,
            "recovery" => Self::Recovery,
            "update" => Self::Update,
            custom => Self::Custom(custom.to_string()),
        })
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Normal => "normal",
            Self::Recovery => "recovery",
            Self::Update => "update",
            Self::Custom(value) => value.as_str(),
        }
    }
}

impl Default for GroupKeyPackagePurpose {
    fn default() -> Self {
        Self::Normal
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GroupMemberRef(String);

impl GroupMemberRef {
    pub fn parse(input: impl AsRef<str>, default_domain: &str) -> crate::ImResult<Self> {
        let value = input.as_ref().trim();
        if value.is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("member".to_string()),
                "group member must not be empty",
            ));
        }
        if value.starts_with("did:") {
            return crate::ids::Did::parse(value).map(Self::from);
        }
        crate::ids::Handle::parse(value, default_domain).map(Self::from)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_did(&self) -> bool {
        self.0.starts_with("did:")
    }

    pub fn as_did(&self) -> crate::ImResult<crate::ids::Did> {
        crate::ids::Did::parse(self.as_str())
    }
}

impl From<crate::ids::Did> for GroupMemberRef {
    fn from(did: crate::ids::Did) -> Self {
        Self(did.as_str().to_string())
    }
}

impl From<crate::ids::Handle> for GroupMemberRef {
    fn from(handle: crate::ids::Handle) -> Self {
        Self(handle.as_str().to_string())
    }
}

impl From<crate::ids::PeerRef> for GroupMemberRef {
    fn from(peer: crate::ids::PeerRef) -> Self {
        Self(peer.as_str().to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupMemberResolution {
    pub did: crate::ids::Did,
    pub handle: Option<crate::ids::Handle>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GroupProfilePatch {
    pub name: Option<String>,
    pub description: Option<String>,
    pub avatar_uri: Option<String>,
    pub discoverability: Option<GroupDiscoverability>,
    pub slug: Option<String>,
    pub goal: Option<String>,
    pub rules: Option<String>,
    pub message_prompt: Option<String>,
    pub doc_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GroupPolicyPatch {
    pub admission_mode: Option<GroupAdmissionMode>,
    pub attachments_allowed: Option<bool>,
    pub max_members: Option<GroupMemberLimit>,
    pub member_max_messages: Option<i64>,
    pub member_max_total_chars: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupDiscoverability {
    Private,
    Public,
    Unlisted,
    Custom(String),
}

impl GroupDiscoverability {
    pub fn parse(input: impl Into<String>) -> crate::ImResult<Self> {
        parse_group_token(input, "discoverability", |value| match value {
            "private" => Self::Private,
            "public" => Self::Public,
            "unlisted" => Self::Unlisted,
            custom => Self::Custom(custom.to_string()),
        })
    }

    pub fn parse_optional(input: impl AsRef<str>) -> crate::ImResult<Option<Self>> {
        parse_optional_group_token(input, Self::parse)
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Private => "private",
            Self::Public => "public",
            Self::Unlisted => "unlisted",
            Self::Custom(value) => value.as_str(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupAdmissionMode {
    OpenJoin,
    InviteOnly,
    ApprovalRequired,
    Closed,
    Custom(String),
}

impl GroupAdmissionMode {
    pub fn parse(input: impl Into<String>) -> crate::ImResult<Self> {
        parse_group_token(input, "admission_mode", |value| match value {
            "open-join" | "open" => Self::OpenJoin,
            "invite-only" => Self::InviteOnly,
            "approval" | "approval-required" => Self::ApprovalRequired,
            "closed" => Self::Closed,
            custom => Self::Custom(custom.to_string()),
        })
    }

    pub fn parse_optional(input: impl AsRef<str>) -> crate::ImResult<Option<Self>> {
        parse_optional_group_token(input, Self::parse)
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::OpenJoin => "open-join",
            Self::InviteOnly => "invite-only",
            Self::ApprovalRequired => "approval",
            Self::Closed => "closed",
            Self::Custom(value) => value.as_str(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupMessageSecurityProfile {
    TransportProtected,
    GroupE2ee,
    Custom(String),
}

impl GroupMessageSecurityProfile {
    pub fn parse(input: impl Into<String>) -> crate::ImResult<Self> {
        parse_group_token(input, "message_security_profile", |value| match value {
            "transport-protected" => Self::TransportProtected,
            "group-e2ee" => Self::GroupE2ee,
            custom => Self::Custom(custom.to_string()),
        })
    }

    pub fn parse_optional(input: impl AsRef<str>) -> crate::ImResult<Option<Self>> {
        parse_optional_group_token(input, Self::parse)
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::TransportProtected => "transport-protected",
            Self::GroupE2ee => "group-e2ee",
            Self::Custom(value) => value.as_str(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupSecurityRequirement {
    #[default]
    Default,
    Required,
}

impl GroupSecurityRequirement {
    pub fn required(self) -> bool {
        matches!(self, Self::Required)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupMemberRole {
    Owner,
    Admin,
    Member,
    Custom(String),
}

impl GroupMemberRole {
    pub fn parse(input: impl Into<String>) -> crate::ImResult<Self> {
        parse_group_token(input, "role", |value| match value {
            "owner" => Self::Owner,
            "admin" => Self::Admin,
            "member" => Self::Member,
            custom => Self::Custom(custom.to_string()),
        })
    }

    pub fn parse_optional(input: impl AsRef<str>) -> crate::ImResult<Option<Self>> {
        parse_optional_group_token(input, Self::parse)
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Owner => "owner",
            Self::Admin => "admin",
            Self::Member => "member",
            Self::Custom(value) => value.as_str(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupMemberLimit(u32);

impl GroupMemberLimit {
    pub fn new(value: u32) -> crate::ImResult<Self> {
        if value == 0 {
            return Err(crate::ImError::invalid_input(
                Some("max_members".to_string()),
                "max_members must be greater than zero",
            ));
        }
        Ok(Self(value))
    }

    pub fn parse(input: impl Into<String>) -> crate::ImResult<Self> {
        let input = input.into();
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("max_members".to_string()),
                "max_members must not be empty",
            ));
        }
        let value = trimmed.parse::<u32>().map_err(|_| {
            crate::ImError::invalid_input(
                Some("max_members".to_string()),
                "max_members must be an unsigned integer",
            )
        })?;
        Self::new(value)
    }

    pub fn parse_optional(input: impl AsRef<str>) -> crate::ImResult<Option<Self>> {
        let trimmed = input.as_ref().trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        Self::parse(trimmed.to_string()).map(Some)
    }

    pub fn as_u32(self) -> u32 {
        self.0
    }

    pub fn to_protocol_string(self) -> String {
        self.0.to_string()
    }
}

#[cfg(test)]
mod group_domain_type_tests {
    use super::*;

    #[test]
    fn group_policy_types_parse_known_and_custom_protocol_values() {
        assert_eq!(
            GroupDiscoverability::parse(" public ").unwrap(),
            GroupDiscoverability::Public
        );
        assert_eq!(
            GroupAdmissionMode::parse("approval-required").unwrap(),
            GroupAdmissionMode::ApprovalRequired
        );
        assert_eq!(GroupAdmissionMode::ApprovalRequired.as_str(), "approval");
        assert_eq!(
            GroupMessageSecurityProfile::parse("group-e2ee").unwrap(),
            GroupMessageSecurityProfile::GroupE2ee
        );
        assert_eq!(
            GroupMemberRole::parse(" moderator ").unwrap(),
            GroupMemberRole::Custom("moderator".to_string())
        );
    }

    #[test]
    fn group_member_limit_rejects_empty_zero_and_non_numeric_values() {
        assert_eq!(GroupMemberLimit::parse(" 25 ").unwrap().as_u32(), 25);
        assert!(GroupMemberLimit::parse("").is_err());
        assert!(GroupMemberLimit::parse("0").is_err());
        assert!(GroupMemberLimit::parse("many").is_err());
    }

    #[test]
    fn group_member_limit_keeps_json_number_and_string_compatibility() {
        let from_number: GroupMemberLimit = serde_json::from_value(serde_json::json!(12)).unwrap();
        let from_string: GroupMemberLimit =
            serde_json::from_value(serde_json::json!("12")).unwrap();

        assert_eq!(from_number, from_string);
        assert_eq!(
            serde_json::to_value(from_number).unwrap(),
            serde_json::json!(12)
        );
    }
}

impl Serialize for GroupDiscoverability {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for GroupDiscoverability {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::parse(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl Serialize for GroupAdmissionMode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for GroupAdmissionMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::parse(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl Serialize for GroupMessageSecurityProfile {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for GroupMessageSecurityProfile {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::parse(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl Serialize for GroupMemberRole {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for GroupMemberRole {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::parse(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl Serialize for GroupKeyPackagePurpose {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for GroupKeyPackagePurpose {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::parse(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl Serialize for GroupMemberLimit {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u32(self.0)
    }
}

impl<'de> Deserialize<'de> for GroupMemberLimit {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct GroupMemberLimitVisitor;

        impl serde::de::Visitor<'_> for GroupMemberLimitVisitor {
            type Value = GroupMemberLimit;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a positive integer or decimal string")
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                let value = u32::try_from(value).map_err(E::custom)?;
                GroupMemberLimit::new(value).map_err(E::custom)
            }

            fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                let value = u32::try_from(value).map_err(E::custom)?;
                GroupMemberLimit::new(value).map_err(E::custom)
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                GroupMemberLimit::parse(value.to_string()).map_err(E::custom)
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                GroupMemberLimit::parse(value).map_err(E::custom)
            }
        }

        deserializer.deserialize_any(GroupMemberLimitVisitor)
    }
}

fn parse_group_token<T>(
    input: impl Into<String>,
    field: &'static str,
    mapper: impl FnOnce(&str) -> T,
) -> crate::ImResult<T> {
    let input = input.into();
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some(field.to_string()),
            format!("{field} must not be empty"),
        ));
    }
    Ok(mapper(trimmed))
}

fn parse_optional_group_token<T>(
    input: impl AsRef<str>,
    parser: impl FnOnce(String) -> crate::ImResult<T>,
) -> crate::ImResult<Option<T>> {
    let trimmed = input.as_ref().trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    parser(trimmed.to_string()).map(Some)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupUpdateProfileRequest {
    pub group: crate::ids::GroupRef,
    pub patch: GroupProfilePatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupUpdatePolicyRequest {
    pub group: crate::ids::GroupRef,
    pub patch: GroupPolicyPatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupUpdateRequest {
    pub group: crate::ids::GroupRef,
    pub profile_patch: GroupProfilePatch,
    pub policy_patch: GroupPolicyPatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupUpdateResult {
    pub deliveries: Vec<GroupReadResult>,
    pub refreshed: Option<GroupReadResult>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupListRequest {
    pub limit: crate::ids::PageLimit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupMembersRequest {
    pub group: crate::ids::GroupRef,
    pub limit: crate::ids::PageLimit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupMessagesRequest {
    pub group: crate::ids::GroupRef,
    pub limit: crate::ids::PageLimit,
    pub cursor: Option<crate::ids::Cursor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupSnapshot {
    pub id: Option<String>,
    pub did: crate::ids::GroupRef,
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub avatar_uri: Option<String>,
    pub my_role: Option<String>,
    pub membership_status: Option<String>,
    pub member_count: Option<u32>,
    pub last_message_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupSummary {
    pub id: Option<String>,
    pub did: crate::ids::GroupRef,
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub avatar_uri: Option<String>,
    pub my_role: Option<String>,
    pub membership_status: Option<String>,
    pub member_count: Option<u32>,
    pub last_message_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupMember {
    pub did: Option<crate::ids::Did>,
    pub handle: Option<crate::ids::Handle>,
    pub role: Option<String>,
    pub status: Option<String>,
    pub joined_at: Option<String>,
    pub subject_type: Option<String>,
}

fn group_snapshot_from_response(raw: &Value) -> Option<GroupSnapshot> {
    if let Some(group) = raw.get("group") {
        return group_snapshot_from_value(group);
    }
    if let Some(group) = raw.get("group_snapshot") {
        return group_snapshot_from_value(group);
    }
    if raw_is_group_snapshot(raw) {
        return group_snapshot_from_value(raw);
    }
    None
}

fn raw_is_group_snapshot(raw: &Value) -> bool {
    let Some(object) = raw.as_object() else {
        return false;
    };
    if object.contains_key("accepted")
        || object.contains_key("final_acceptance")
        || object.contains_key("operation_id")
        || object.contains_key("group_receipt")
        || object.contains_key("member_did")
        || object.contains_key("leaver_did")
    {
        return false;
    }
    object.contains_key("group_profile")
        || object.contains_key("name")
        || object.contains_key("display_name")
        || object.contains_key("description")
        || object.contains_key("member_role")
        || object.contains_key("my_role")
        || object.contains_key("actor_membership_role")
        || object.contains_key("member_status")
        || object.contains_key("actor_membership_status")
}

fn group_snapshot_from_value(value: &Value) -> Option<GroupSnapshot> {
    let object = value.as_object()?;
    let did = group_ref_from_object(object)?;
    let display_name = optional_string(object.get("display_name"))
        .or_else(|| nested_string(object.get("group_profile"), "display_name"))
        .or_else(|| optional_string(object.get("name")));
    Some(GroupSnapshot {
        id: optional_string(object.get("id")).or_else(|| optional_string(object.get("group_id"))),
        did,
        name: display_name.clone(),
        display_name,
        description: optional_string(object.get("description"))
            .or_else(|| nested_string(object.get("group_profile"), "description")),
        avatar_uri: optional_string(object.get("avatar_uri"))
            .or_else(|| nested_string(object.get("group_profile"), "avatar_uri"))
            .or_else(|| optional_string(object.get("avatar_url")))
            .or_else(|| optional_string(object.get("avatar"))),
        my_role: optional_string(object.get("my_role"))
            .or_else(|| optional_string(object.get("member_role")))
            .or_else(|| optional_string(object.get("actor_membership_role"))),
        membership_status: optional_string(object.get("membership_status"))
            .or_else(|| optional_string(object.get("member_status")))
            .or_else(|| optional_string(object.get("actor_membership_status")))
            .or_else(|| optional_string(object.get("status"))),
        member_count: u32_value(object.get("member_count")),
        last_message_at: optional_string(object.get("last_message_at")),
    })
}

fn group_summary_from_value(value: Value) -> Option<GroupSummary> {
    let object = value.as_object()?;
    let did = group_ref_from_object(object)?;
    let display_name = optional_string(object.get("display_name"))
        .or_else(|| nested_string(object.get("group_profile"), "display_name"))
        .or_else(|| optional_string(object.get("name")));
    Some(GroupSummary {
        id: optional_string(object.get("id")).or_else(|| optional_string(object.get("group_id"))),
        did,
        name: display_name.clone(),
        display_name,
        avatar_uri: optional_string(object.get("avatar_uri"))
            .or_else(|| nested_string(object.get("group_profile"), "avatar_uri"))
            .or_else(|| optional_string(object.get("avatar_url")))
            .or_else(|| optional_string(object.get("avatar"))),
        my_role: optional_string(object.get("my_role"))
            .or_else(|| optional_string(object.get("member_role")))
            .or_else(|| optional_string(object.get("actor_membership_role"))),
        membership_status: optional_string(object.get("membership_status"))
            .or_else(|| optional_string(object.get("member_status")))
            .or_else(|| optional_string(object.get("status"))),
        member_count: u32_value(object.get("member_count")),
        last_message_at: optional_string(object.get("last_message_at")),
    })
}

fn group_member_from_value(value: Value) -> Option<GroupMember> {
    let object = value.as_object()?;
    let did = optional_string(object.get("did"))
        .or_else(|| optional_string(object.get("member_did")))
        .or_else(|| optional_string(object.get("agent_did")))
        .and_then(|value| crate::ids::Did::parse(value).ok());
    let subject_type = optional_string(object.get("subject_type"))
        .or_else(|| optional_string(object.get("subjectType")))
        .or_else(|| optional_string(object.get("member_subject_type")))
        .or_else(|| optional_string(object.get("agent_subject_type")))
        .or_else(|| inferred_subject_type(did.as_ref(), object));
    Some(GroupMember {
        did,
        handle: optional_string(object.get("handle"))
            .or_else(|| optional_string(object.get("member_handle")))
            .or_else(|| optional_string(object.get("agent_handle")))
            .and_then(|value| crate::ids::Handle::parse(value, "").ok()),
        role: optional_string(object.get("role")),
        status: optional_string(object.get("status")),
        joined_at: optional_string(object.get("joined_at")),
        subject_type,
    })
}

fn inferred_subject_type(
    did: Option<&crate::ids::Did>,
    object: &serde_json::Map<String, Value>,
) -> Option<String> {
    if optional_string(object.get("agent_did")).is_some()
        || optional_string(object.get("agent_handle")).is_some()
    {
        return Some("agent".to_string());
    }
    let did = did?.as_str().trim();
    if did.starts_with("did:agent:") {
        return Some("agent".to_string());
    }
    if did.starts_with("did:") {
        return Some("human".to_string());
    }
    None
}

fn group_message_from_value(value: Value) -> Option<crate::messages::Message> {
    let object = value.as_object()?;
    let id = optional_string(object.get("id"))
        .or_else(|| optional_string(object.get("message_id")))
        .or_else(|| optional_string(object.get("msg_id")))?;
    let group_did = optional_string(object.get("group_did"))
        .or_else(|| optional_string(object.get("group")))
        .unwrap_or_else(|| "group:unknown".to_string());
    let sender = optional_string(object.get("sender_did"))
        .unwrap_or_else(|| "did:unknown:sender".to_string());
    let content_type = optional_string(object.get("content_type"));
    let secure = object
        .get("secure")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let is_attachment_manifest = content_type.as_deref()
        == Some(crate::attachments::manifest::attachment_manifest_content_type());
    let body = if content_type.as_deref() == Some("application/json") || is_attachment_manifest {
        object
            .get("payload")
            .or_else(|| object.get("content"))
            .or_else(|| object.get("body").and_then(|body| body.get("payload")))
            .cloned()
            .filter(Value::is_object)
            .map(|payload| crate::messages::MessageBodyView::Payload { payload })
            .unwrap_or(crate::messages::MessageBodyView::Unsupported {
                content_type: content_type.clone(),
            })
    } else if let Some(text) = optional_string(object.get("text"))
        .or_else(|| optional_string(object.get("content")))
        .or_else(|| nested_string(object.get("body"), "text"))
    {
        crate::messages::MessageBodyView::Text {
            text,
            kind: message_kind(content_type.as_deref()),
        }
    } else {
        crate::messages::MessageBodyView::Unsupported {
            content_type: content_type.clone(),
        }
    };
    let group = crate::ids::GroupRef::parse(&group_did).ok()?;
    Some(crate::messages::Message {
        id: crate::ids::MessageId::parse(id).ok()?,
        thread: crate::messages::ThreadRef::Group(group.clone()),
        direction: crate::messages::MessageDirection::Unknown,
        sender: crate::ids::PeerRef::parse(sender, "").ok()?,
        receiver: None,
        group: Some(group),
        body,
        sent_at: optional_string(object.get("sent_at"))
            .or_else(|| optional_string(object.get("created_at"))),
        received_at: optional_string(object.get("received_at")),
        metadata: crate::messages::MessageMetadata {
            operation_id: optional_string(object.get("operation_id")),
            delivery_state: optional_string(object.get("delivery_state")),
            server_sequence: i64_value(object.get("server_seq"))
                .or_else(|| i64_value(object.get("sequence")))
                .or_else(|| i64_value(object.get("group_event_seq"))),
            content_type,
            attributes: group_message_attributes(object, secure),
            ..crate::messages::MessageMetadata::default()
        },
    })
}

fn group_message_attributes(
    object: &serde_json::Map<String, Value>,
    secure: bool,
) -> Vec<crate::messages::MessageMetadataAttribute> {
    let mut attributes = Vec::new();
    if secure {
        attributes.push(crate::messages::MessageMetadataAttribute {
            key: "security".to_owned(),
            value: "group-e2ee".to_owned(),
        });
        attributes.push(crate::messages::MessageMetadataAttribute {
            key: "message_security_profile".to_owned(),
            value: "group-e2ee".to_owned(),
        });
    }
    for key in [
        "decryption_state",
        "secure_wire_content_type",
        "type",
        "message_security_profile",
        "security_profile",
    ] {
        if let Some(value) = optional_string(object.get(key)) {
            if attributes
                .iter()
                .any(|attribute| attribute.key == key && attribute.value == value)
            {
                continue;
            }
            attributes.push(crate::messages::MessageMetadataAttribute {
                key: key.to_owned(),
                value,
            });
        }
    }
    if let Some(content_type) = optional_string(object.get("content_type")) {
        attributes.push(crate::messages::MessageMetadataAttribute {
            key: "content_type".to_owned(),
            value: content_type,
        });
    }
    attributes
}

fn group_ref_from_object(object: &serde_json::Map<String, Value>) -> Option<crate::ids::GroupRef> {
    optional_string(object.get("group_did"))
        .or_else(|| optional_string(object.get("did")))
        .or_else(|| optional_string(object.get("id")))
        .and_then(|value| crate::ids::GroupRef::parse(value).ok())
}

fn values_from_array(value: Option<&Value>) -> Vec<Value> {
    value
        .and_then(Value::as_array)
        .map(|items| items.to_vec())
        .unwrap_or_default()
}

fn optional_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn nested_string(value: Option<&Value>, key: &str) -> Option<String> {
    value
        .and_then(Value::as_object)
        .and_then(|object| optional_string(object.get(key)))
}

fn cursor_from_value(value: Option<&Value>) -> Option<crate::ids::Cursor> {
    optional_string(value).and_then(|value| crate::ids::Cursor::parse(value).ok())
}

fn u32_value(value: Option<&Value>) -> Option<u32> {
    value
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
}

fn i64_value(value: Option<&Value>) -> Option<i64> {
    value.and_then(Value::as_i64)
}

fn bool_value(value: Option<&Value>) -> bool {
    value.and_then(Value::as_bool).unwrap_or(false)
}

fn merge_raw_warnings(raw_warnings: Option<&Value>, mut warnings: Vec<String>) -> Vec<String> {
    let Some(items) = raw_warnings.and_then(Value::as_array) else {
        return warnings;
    };
    for item in items {
        if let Some(warning) = item
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let warning = warning.to_owned();
            if !warnings.iter().any(|known| known == &warning) {
                warnings.push(warning);
            }
        }
    }
    warnings
}

fn message_kind(content_type: Option<&str>) -> crate::messages::MessageKind {
    match content_type.map(str::trim) {
        Some("text/markdown" | "markdown" | "text/x-markdown") => {
            crate::messages::MessageKind::Markdown
        }
        _ => crate::messages::MessageKind::Text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn group_create_request_new_sets_only_required_name() {
        let request = GroupCreateRequest::new("Demo Group");

        assert_eq!(request.name, "Demo Group");
        assert_eq!(request.description, None);
        assert_eq!(request.avatar_uri, None);
        assert_eq!(request.security, GroupSecurityRequirement::default());
        assert!(!request.e2ee);
        assert_eq!(request.discoverability, None);
        assert_eq!(request.admission_mode, None);
        assert_eq!(request.message_security_profile, None);
    }

    #[test]
    fn group_result_projects_domain_fields_and_keeps_raw_response() {
        let result = GroupReadResult::from_raw_response(
            json!({
                "group_did": "did:example:group",
                "name": "Demo",
                "membership_status": "active",
                "member_count": 2,
                "groups": [{
                    "group_did": "did:example:group",
                    "name": "Demo",
                    "membership_status": "active"
                }],
                "members": [{
                    "member_did": "did:example:bob",
                    "handle": "bob.example",
                    "role": "member",
                    "status": "active"
                }],
                "messages": [{
                    "id": "msg-1",
                    "group_did": "did:example:group",
                    "sender_did": "did:example:bob",
                    "text": "hello",
                    "sent_at": "2026-01-01T00:00:00Z"
                }],
                "total": 1,
                "has_more": false,
                "source": "remote_http"
            }),
            vec!["normalized".to_string()],
        );

        let group = result.group.as_ref().expect("group snapshot");
        assert_eq!(group.did.as_str(), "did:example:group");
        assert_eq!(group.name.as_deref(), Some("Demo"));
        assert_eq!(group.member_count, Some(2));
        assert_eq!(result.groups[0].did.as_str(), "did:example:group");
        assert_eq!(
            result.members[0].did.as_ref().map(crate::ids::Did::as_str),
            Some("did:example:bob")
        );
        assert_eq!(result.messages.items[0].id.as_str(), "msg-1");
        assert_eq!(result.total, Some(1));
        assert_eq!(result.source.as_deref(), Some("remote_http"));
        assert_eq!(result.warnings, vec!["normalized"]);
        assert_eq!(
            result.raw_response().and_then(|raw| raw.get("group_did")),
            Some(&json!("did:example:group"))
        );
    }

    #[test]
    fn group_member_subject_type_ignores_empty_agent_fields() {
        let result = GroupReadResult::from_raw_response(
            json!({
                "group_did": "did:example:group",
                "name": "Demo",
                "members": [{
                    "member_did": "did:wba:awiki.info:user:zhuocheng:e1",
                    "handle": "zhuocheng",
                    "agent_handle": "",
                    "role": "member",
                    "status": "active"
                }, {
                    "agent_did": "did:wba:awiki.info:agent:runtime:hermes:e1",
                    "agent_handle": "hermes.awiki.info",
                    "role": "member",
                    "status": "active"
                }]
            }),
            Vec::new(),
        );

        assert_eq!(result.members.len(), 2);
        assert_eq!(result.members[0].subject_type.as_deref(), Some("human"));
        assert_eq!(result.members[1].subject_type.as_deref(), Some("agent"));
    }

    #[test]
    fn group_result_does_not_treat_member_mutation_response_as_viewer_snapshot() {
        let result = GroupReadResult::from_raw_response(
            json!({
                "accepted": true,
                "final_acceptance": true,
                "group_did": "did:example:group",
                "group_state_version": "12",
                "group_event_seq": "7",
                "operation_id": "op-remove-bob",
                "member_did": "did:example:bob",
                "membership_status": "removed"
            }),
            Vec::new(),
        );

        assert!(
            result.group.is_none(),
            "target member status in a group.remove response must not be read as the viewer membership status"
        );
        assert_eq!(
            result
                .raw_response()
                .and_then(|raw| raw.get("membership_status")),
            Some(&json!("removed"))
        );
    }

    #[test]
    fn group_result_still_projects_explicit_group_snapshot_wrapper() {
        let result = GroupReadResult::from_raw_response(
            json!({
                "group": {
                    "group_did": "did:example:group",
                    "name": "Demo",
                    "member_role": "owner",
                    "membership_status": "active",
                    "member_count": 2
                }
            }),
            Vec::new(),
        );

        let group = result.group.as_ref().expect("group snapshot");
        assert_eq!(group.did.as_str(), "did:example:group");
        assert_eq!(group.my_role.as_deref(), Some("owner"));
        assert_eq!(group.membership_status.as_deref(), Some("active"));
    }

    #[test]
    fn group_result_still_projects_local_group_snapshot_wrapper() {
        let result = GroupReadResult::from_raw_response(
            json!({
                "group_snapshot": {
                    "group_did": "did:example:group",
                    "name": "Demo",
                    "member_role": "admin",
                    "member_status": "active",
                    "member_count": 3
                }
            }),
            Vec::new(),
        );

        let group = result.group.as_ref().expect("group snapshot");
        assert_eq!(group.did.as_str(), "did:example:group");
        assert_eq!(group.my_role.as_deref(), Some("admin"));
        assert_eq!(group.membership_status.as_deref(), Some("active"));
    }

    #[test]
    fn group_result_merges_raw_warnings() {
        let result = GroupReadResult::from_raw_response(
            json!({
                "messages": [],
                "warnings": [
                    "Failed to decrypt group E2EE message did:example:group:3: aad_mismatch",
                    "",
                    42
                ],
                "has_more": false
            }),
            vec![
                "existing warning".to_owned(),
                "Failed to decrypt group E2EE message did:example:group:3: aad_mismatch".to_owned(),
            ],
        );

        assert_eq!(
            result.warnings,
            vec![
                "existing warning",
                "Failed to decrypt group E2EE message did:example:group:3: aad_mismatch",
            ]
        );
    }

    #[test]
    fn group_result_projects_secure_attachment_manifest_payload() {
        let result = GroupReadResult::from_raw_response(
            json!({
                "messages": [{
                    "id": "msg-secure-attachment",
                    "group_did": "did:example:group",
                    "sender_did": "did:example:alice",
                    "type": "attachment_manifest",
                    "content_type": crate::attachments::manifest::attachment_manifest_content_type(),
                    "secure": true,
                    "content": {
                        "attachments": [{
                            "attachment_id": "att-group-secure",
                            "size": "48",
                            "digest": {
                                "alg": "sha-256",
                                "value_b64u": "digest"
                            },
                            "mime_type": "text/plain",
                            "encryption_info": {
                                "mode": "object-e2ee",
                                "alg": "chacha20-poly1305",
                                "plaintext_size": "31",
                                "object_uri": "https://objects.example/secure"
                            }
                        }],
                        "caption": "secure attachment",
                        "primary_attachment_id": "att-group-secure"
                    },
                    "sent_at": "2026-01-01T00:00:00Z"
                }],
                "has_more": false
            }),
            Vec::new(),
        );

        let message = &result.messages.items[0];
        assert!(matches!(
            &message.body,
            crate::messages::MessageBodyView::Payload { payload }
                if payload["attachments"][0]["attachment_id"] == "att-group-secure"
                    && payload["attachments"][0]["encryption_info"]["mode"] == "object-e2ee"
                    && payload["attachments"][0]["encryption_info"].get("object_key_b64u").is_none()
                    && payload["attachments"][0]["encryption_info"].get("nonce_b64u").is_none()
        ));
        assert!(message
            .metadata
            .attributes
            .iter()
            .any(|attribute| attribute.key == "security" && attribute.value == "group-e2ee"));
        assert!(message.metadata.attributes.iter().any(|attribute| {
            attribute.key == "message_security_profile" && attribute.value == "group-e2ee"
        }));
        assert!(message
            .metadata
            .attributes
            .iter()
            .any(|attribute| attribute.key == "type" && attribute.value == "attachment_manifest"));
        assert!(message.metadata.attributes.iter().any(|attribute| {
            attribute.key == "content_type"
                && attribute.value
                    == crate::attachments::manifest::attachment_manifest_content_type()
        }));
    }
}
