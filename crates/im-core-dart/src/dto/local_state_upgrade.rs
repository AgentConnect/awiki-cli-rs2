#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartLocalStateUpgradeEligibility {
    NotRequired,
    Required,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartLocalStateUpgradeInspection {
    pub eligibility: DartLocalStateUpgradeEligibility,
    pub source_schema_version: i64,
    pub target_schema_version: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DartLocalStateUpgradeStatus {
    NotRequired,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartLocalStateUpgradeResult {
    pub status: DartLocalStateUpgradeStatus,
    pub source_schema_version: i64,
    pub target_schema_version: i64,
    pub migrated_personas: u64,
    pub migrated_conversations: u64,
    pub unresolved_messages: u64,
    pub alias_count: u64,
    pub backup_available: bool,
    pub alias_mappings: Vec<DartLocalStateConversationAliasMapping>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartLocalStateConversationAliasMapping {
    pub owner_identity_id: String,
    pub owner_did: String,
    pub legacy_conversation_id: String,
    pub canonical_conversation_id: String,
}

impl From<im_core::LocalStateUpgradeInspection> for DartLocalStateUpgradeInspection {
    fn from(value: im_core::LocalStateUpgradeInspection) -> Self {
        Self {
            eligibility: match value.eligibility {
                im_core::LocalStateUpgradeEligibility::NotRequired => {
                    DartLocalStateUpgradeEligibility::NotRequired
                }
                im_core::LocalStateUpgradeEligibility::Required => {
                    DartLocalStateUpgradeEligibility::Required
                }
            },
            source_schema_version: value.source_schema_version,
            target_schema_version: value.target_schema_version,
        }
    }
}

impl From<im_core::LocalStateUpgradeResult> for DartLocalStateUpgradeResult {
    fn from(value: im_core::LocalStateUpgradeResult) -> Self {
        Self {
            status: match value.status {
                im_core::LocalStateUpgradeStatus::NotRequired => {
                    DartLocalStateUpgradeStatus::NotRequired
                }
                im_core::LocalStateUpgradeStatus::Completed => {
                    DartLocalStateUpgradeStatus::Completed
                }
            },
            source_schema_version: value.source_schema_version,
            target_schema_version: value.target_schema_version,
            migrated_personas: value.migrated_personas,
            migrated_conversations: value.migrated_conversations,
            unresolved_messages: value.unresolved_messages,
            alias_count: value.alias_count,
            backup_available: value.backup_available,
            alias_mappings: value
                .alias_mappings
                .into_iter()
                .map(|mapping| DartLocalStateConversationAliasMapping {
                    owner_identity_id: mapping.owner_identity_id,
                    owner_did: mapping.owner_did,
                    legacy_conversation_id: mapping.legacy_conversation_id,
                    canonical_conversation_id: mapping.canonical_conversation_id,
                })
                .collect(),
        }
    }
}
