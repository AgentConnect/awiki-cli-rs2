use rusqlite::Connection;
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) const SCHEMA_VERSION: i64 = 29;
pub(crate) const IDENTITY_OWNED_SCHEMA_VERSION: i64 = 17;
const CONVERSATION_SUMMARIES_SCHEMA_VERSION: i64 = 27;
const CONVERSATION_REGISTRY_SCHEMA_VERSION: i64 = 26;
const SYSTEM_NOTIFICATION_SCHEMA_VERSION: i64 = 29;

const V6_TABLES_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS contacts (
    owner_identity_id TEXT,
    owner_did       TEXT NOT NULL DEFAULT '',
    did             TEXT NOT NULL,
    name            TEXT,
    handle          TEXT,
    nick_name       TEXT,
    bio             TEXT,
    profile_md      TEXT,
    tags            TEXT,
    relationship    TEXT,
    source_type     TEXT,
    source_name     TEXT,
    source_group_id TEXT,
    connected_at    TEXT,
    recommended_reason TEXT,
    followed        INTEGER NOT NULL DEFAULT 0,
    messaged        INTEGER NOT NULL DEFAULT 0,
    note            TEXT,
    first_seen_at   TEXT,
    last_seen_at    TEXT,
    metadata        TEXT,
    credential_name TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (owner_did, did)
);

CREATE TABLE IF NOT EXISTS messages (
    msg_id          TEXT NOT NULL,
    owner_identity_id TEXT,
    owner_did       TEXT NOT NULL DEFAULT '',
    thread_id       TEXT NOT NULL,
    direction       INTEGER NOT NULL DEFAULT 0,
    sender_did      TEXT,
    receiver_did    TEXT,
    group_id        TEXT,
    group_did       TEXT,
    content_type    TEXT DEFAULT 'text',
    content         TEXT,
    title           TEXT,
    server_seq      INTEGER,
    sent_at         TEXT,
    stored_at       TEXT NOT NULL,
    is_e2ee         INTEGER DEFAULT 0,
    is_read         INTEGER DEFAULT 0,
    sender_name     TEXT,
    metadata        TEXT,
    mentions_current_user INTEGER NOT NULL DEFAULT 0,
    credential_name TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (msg_id, owner_did)
);

CREATE TABLE IF NOT EXISTS e2ee_outbox (
    outbox_id            TEXT PRIMARY KEY,
    owner_identity_id    TEXT,
    owner_did            TEXT NOT NULL DEFAULT '',
    peer_did             TEXT NOT NULL,
    session_id           TEXT,
    original_type        TEXT NOT NULL DEFAULT 'text',
    plaintext            TEXT NOT NULL,
    local_status         TEXT NOT NULL DEFAULT 'queued',
    attempt_count        INTEGER NOT NULL DEFAULT 0,
    sent_msg_id          TEXT,
    sent_server_seq      INTEGER,
    last_error_code      TEXT,
    retry_hint           TEXT,
    failed_msg_id        TEXT,
    failed_server_seq    INTEGER,
    metadata             TEXT,
    last_attempt_at      TEXT,
    created_at           TEXT NOT NULL,
    updated_at           TEXT NOT NULL,
    credential_name      TEXT NOT NULL DEFAULT ''
);
"#;

const V7_TABLES_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS groups (
    owner_identity_id TEXT,
    owner_did          TEXT NOT NULL DEFAULT '',
    group_id           TEXT NOT NULL,
    group_did          TEXT,
    name               TEXT,
    group_mode         TEXT NOT NULL DEFAULT 'general',
    slug               TEXT,
    description        TEXT,
    goal               TEXT,
    rules              TEXT,
    message_prompt     TEXT,
    doc_url            TEXT,
    group_owner_did    TEXT,
    group_owner_handle TEXT,
    my_role            TEXT,
    membership_status  TEXT NOT NULL DEFAULT 'active',
    join_enabled       INTEGER,
    join_code          TEXT,
    join_code_expires_at TEXT,
    member_count       INTEGER,
    last_synced_seq    INTEGER,
    last_read_seq      INTEGER,
    last_message_at    TEXT,
    remote_created_at  TEXT,
    remote_updated_at  TEXT,
    stored_at          TEXT NOT NULL,
    metadata           TEXT,
    credential_name    TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (owner_did, group_id)
);

CREATE TABLE IF NOT EXISTS group_members (
    owner_identity_id TEXT,
    owner_did         TEXT NOT NULL DEFAULT '',
    group_id          TEXT NOT NULL,
    user_id           TEXT NOT NULL,
    member_did        TEXT,
    member_handle     TEXT,
    anchor_kind       TEXT NOT NULL DEFAULT 'did',
    anchor_value      TEXT NOT NULL DEFAULT '',
    handle_binding_generation TEXT,
    profile_url       TEXT,
    role              TEXT,
    status            TEXT NOT NULL DEFAULT 'active',
    joined_at         TEXT,
    sent_message_count INTEGER NOT NULL DEFAULT 0,
    last_synced_at    TEXT NOT NULL,
    metadata          TEXT,
    credential_name   TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (owner_did, group_id, user_id)
);
"#;

const V8_TABLES_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS relationship_events (
    event_id         TEXT PRIMARY KEY,
    owner_identity_id TEXT,
    owner_did        TEXT NOT NULL DEFAULT '',
    target_did       TEXT NOT NULL,
    target_handle    TEXT,
    event_type       TEXT NOT NULL,
    source_type      TEXT,
    source_name      TEXT,
    source_group_id  TEXT,
    reason           TEXT,
    score            REAL,
    status           TEXT NOT NULL DEFAULT 'pending',
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL,
    metadata         TEXT,
    credential_name  TEXT NOT NULL DEFAULT ''
);
"#;

const V11_TABLES_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS e2ee_sessions (
    owner_did        TEXT NOT NULL DEFAULT '',
    peer_did         TEXT NOT NULL,
    session_id       TEXT NOT NULL,
    is_initiator     INTEGER NOT NULL DEFAULT 0,
    send_chain_key   TEXT NOT NULL,
    recv_chain_key   TEXT NOT NULL,
    send_seq         INTEGER NOT NULL DEFAULT 0,
    recv_seq         INTEGER NOT NULL DEFAULT 0,
    expires_at       REAL,
    created_at       TEXT NOT NULL,
    active_at        TEXT,
    peer_confirmed   INTEGER NOT NULL DEFAULT 0,
    credential_name  TEXT NOT NULL DEFAULT '',
    updated_at       TEXT NOT NULL,
    PRIMARY KEY (owner_did, peer_did),
    UNIQUE (owner_did, session_id)
);
"#;

const V12_TABLES_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS contact_handle_bindings (
    owner_identity_id TEXT,
    owner_did        TEXT NOT NULL DEFAULT '',
    handle           TEXT NOT NULL,
    did              TEXT NOT NULL,
    is_current       INTEGER NOT NULL DEFAULT 1,
    first_seen_at    TEXT NOT NULL,
    last_seen_at     TEXT NOT NULL,
    source_type      TEXT,
    source_group_id  TEXT,
    metadata         TEXT,
    credential_name  TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (owner_did, handle, did)
);
"#;

const V14_TABLES_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS direct_e2ee_sessions (
    owner_identity_id TEXT NOT NULL,
    owner_did         TEXT NOT NULL DEFAULT '',
    peer_did          TEXT NOT NULL,
    session_id        TEXT NOT NULL,
    state_blob        BLOB NOT NULL,
    metadata_json     TEXT,
    revision          INTEGER NOT NULL DEFAULT 0,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    PRIMARY KEY (owner_identity_id, peer_did),
    UNIQUE (owner_identity_id, session_id)
);

CREATE TABLE IF NOT EXISTS direct_e2ee_signed_prekeys (
    owner_identity_id TEXT NOT NULL,
    owner_did         TEXT NOT NULL DEFAULT '',
    key_id            TEXT NOT NULL,
    private_key_blob  BLOB NOT NULL,
    public_key_blob   BLOB,
    status            TEXT NOT NULL DEFAULT 'active',
    metadata_json     TEXT,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    PRIMARY KEY (owner_identity_id, key_id)
);

CREATE TABLE IF NOT EXISTS direct_e2ee_one_time_prekeys (
    owner_identity_id TEXT NOT NULL,
    owner_did         TEXT NOT NULL DEFAULT '',
    key_id            TEXT NOT NULL,
    private_key_blob  BLOB NOT NULL,
    public_key_blob   BLOB,
    status            TEXT NOT NULL DEFAULT 'available',
    metadata_json     TEXT,
    created_at        TEXT NOT NULL,
    consumed_at       TEXT,
    PRIMARY KEY (owner_identity_id, key_id)
);
"#;

const ATTACHMENT_MANIFEST_CACHE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS attachment_manifest_cache (
    owner_identity_id       TEXT NOT NULL,
    owner_did               TEXT NOT NULL DEFAULT '',
    thread_kind             TEXT NOT NULL,
    thread_id               TEXT NOT NULL,
    message_id              TEXT NOT NULL,
    wire_message_id         TEXT NOT NULL DEFAULT '',
    sender_did              TEXT,
    message_security_profile TEXT NOT NULL DEFAULT 'transport-protected',
    content                 TEXT NOT NULL,
    stored_at               TEXT NOT NULL,
    PRIMARY KEY (owner_identity_id, thread_kind, thread_id, message_id)
);
"#;

const SYNC_STATE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS sync_state (
    owner_identity_id TEXT NOT NULL,
    owner_did         TEXT NOT NULL DEFAULT '',
    scope             TEXT NOT NULL,
    checkpoint_kind   TEXT NOT NULL,
    event_seq         TEXT NOT NULL DEFAULT '0',
    updated_at        TEXT NOT NULL,
    metadata_json     TEXT,
    PRIMARY KEY (owner_identity_id, scope, checkpoint_kind)
);

CREATE INDEX IF NOT EXISTS idx_sync_state_owner_kind
ON sync_state(owner_identity_id, checkpoint_kind, updated_at DESC);
"#;

const THREAD_READ_STATE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS thread_read_state (
    owner_identity_id          TEXT NOT NULL,
    owner_did                  TEXT NOT NULL DEFAULT '',
    thread_scope               TEXT NOT NULL,
    thread_id                  TEXT NOT NULL,
    conversation_id            TEXT NOT NULL DEFAULT '',
    read_watermark_message_id  TEXT,
    read_watermark_seq         TEXT,
    read_watermark_at          TEXT,
    pending_remote_ack         INTEGER NOT NULL DEFAULT 0,
    remote_ack_at              TEXT,
    updated_at                 TEXT NOT NULL,
    PRIMARY KEY (owner_identity_id, thread_scope, thread_id)
);

CREATE INDEX IF NOT EXISTS idx_thread_read_state_owner_pending
ON thread_read_state(owner_identity_id, pending_remote_ack, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_thread_read_state_owner_conversation
ON thread_read_state(owner_identity_id, conversation_id);
"#;

pub(crate) const MESSAGE_IDENTITY_ALIASES_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS message_identity_aliases (
    owner_identity_id TEXT NOT NULL,
    alias_msg_id      TEXT NOT NULL,
    canonical_msg_id  TEXT NOT NULL,
    source            TEXT NOT NULL DEFAULT '',
    stored_at         TEXT NOT NULL,
    PRIMARY KEY (owner_identity_id, alias_msg_id)
);

CREATE INDEX IF NOT EXISTS idx_message_identity_aliases_owner_canonical
ON message_identity_aliases(owner_identity_id, canonical_msg_id);
"#;

const DIRECT_PEER_ROUTES_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS direct_peer_routes (
    owner_identity_id TEXT NOT NULL,
    conversation_id   TEXT NOT NULL,
    peer_persona_id   TEXT,
    authority_namespace TEXT,
    peer_user_id      TEXT NOT NULL,
    full_handle       TEXT NOT NULL,
    current_did       TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    PRIMARY KEY (owner_identity_id, conversation_id),
    UNIQUE (owner_identity_id, peer_user_id, full_handle)
);

CREATE INDEX IF NOT EXISTS idx_direct_peer_routes_owner_did
ON direct_peer_routes(owner_identity_id, current_did);
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IdentityOwnedSchemaTableMode {
    Final,
    RebuildNew,
}

impl IdentityOwnedSchemaTableMode {
    fn suffix(self) -> &'static str {
        match self {
            Self::Final => "",
            Self::RebuildNew => "_new",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalStateOwnerHint {
    pub(crate) owner_identity_id: String,
    pub(crate) current_did: String,
    pub(crate) historical_dids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RebuildRowOwnershipInput {
    pub(crate) table: &'static str,
    pub(crate) row_key: String,
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) credential_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RebuildRowOwnerResolution {
    Resolved(crate::internal::local_state::owner_scope::OwnerScope),
    Unresolved(RedactedRebuildRow),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RedactedRebuildRow {
    pub(crate) table: &'static str,
    pub(crate) row_key: String,
    pub(crate) reason: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IdentityOwnedMergeTarget {
    Messages,
    Contacts,
    ContactHandleBindings,
    Groups,
    GroupMembers,
    RelationshipEvents,
    E2eeOutbox,
    AttachmentManifestCache,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IdentityOwnedMergeSpec {
    pub(crate) target: IdentityOwnedMergeTarget,
    pub(crate) table: &'static str,
    pub(crate) staging_table: &'static str,
    pub(crate) key_columns: &'static [&'static str],
    pub(crate) order_columns: &'static [&'static str],
}

const IDENTITY_OWNED_MERGE_SPECS: &[IdentityOwnedMergeSpec] = &[
    IdentityOwnedMergeSpec {
        target: IdentityOwnedMergeTarget::Messages,
        table: "messages",
        staging_table: "messages_new",
        key_columns: &["owner_identity_id", "msg_id"],
        order_columns: &["stored_at", "sent_at", "msg_id"],
    },
    IdentityOwnedMergeSpec {
        target: IdentityOwnedMergeTarget::Contacts,
        table: "contacts",
        staging_table: "contacts_new",
        key_columns: &["owner_identity_id", "did"],
        order_columns: &["last_seen_at", "first_seen_at", "did"],
    },
    IdentityOwnedMergeSpec {
        target: IdentityOwnedMergeTarget::ContactHandleBindings,
        table: "contact_handle_bindings",
        staging_table: "contact_handle_bindings_new",
        key_columns: &["owner_identity_id", "handle", "did"],
        order_columns: &["last_seen_at", "first_seen_at", "did"],
    },
    IdentityOwnedMergeSpec {
        target: IdentityOwnedMergeTarget::Groups,
        table: "groups",
        staging_table: "groups_new",
        key_columns: &["owner_identity_id", "group_id"],
        order_columns: &["remote_updated_at", "stored_at", "group_id"],
    },
    IdentityOwnedMergeSpec {
        target: IdentityOwnedMergeTarget::GroupMembers,
        table: "group_members",
        staging_table: "group_members_new",
        key_columns: &["owner_identity_id", "group_id", "user_id"],
        order_columns: &["last_synced_at", "group_id", "user_id"],
    },
    IdentityOwnedMergeSpec {
        target: IdentityOwnedMergeTarget::RelationshipEvents,
        table: "relationship_events",
        staging_table: "relationship_events_new",
        key_columns: &["owner_identity_id", "event_id"],
        order_columns: &["updated_at", "created_at", "event_id"],
    },
    IdentityOwnedMergeSpec {
        target: IdentityOwnedMergeTarget::E2eeOutbox,
        table: "e2ee_outbox",
        staging_table: "e2ee_outbox_new",
        key_columns: &["owner_identity_id", "outbox_id"],
        order_columns: &["updated_at", "created_at", "outbox_id"],
    },
    IdentityOwnedMergeSpec {
        target: IdentityOwnedMergeTarget::AttachmentManifestCache,
        table: "attachment_manifest_cache",
        staging_table: "attachment_manifest_cache_new",
        key_columns: &[
            "owner_identity_id",
            "thread_kind",
            "thread_id",
            "message_id",
        ],
        order_columns: &["stored_at", "message_id"],
    },
];

#[derive(Debug, Clone, Copy)]
struct IdentityOwnedKeySpec {
    table: &'static str,
    key_columns: &'static [&'static str],
}

const OWNER_KEY_SPECS_V17: &[IdentityOwnedKeySpec] = &[
    IdentityOwnedKeySpec {
        table: "contacts",
        key_columns: &["did"],
    },
    IdentityOwnedKeySpec {
        table: "contact_handle_bindings",
        key_columns: &["handle", "did"],
    },
    IdentityOwnedKeySpec {
        table: "messages",
        key_columns: &["msg_id"],
    },
    IdentityOwnedKeySpec {
        table: "groups",
        key_columns: &["group_id"],
    },
    IdentityOwnedKeySpec {
        table: "group_members",
        key_columns: &["group_id", "user_id"],
    },
    IdentityOwnedKeySpec {
        table: "relationship_events",
        key_columns: &["event_id"],
    },
    IdentityOwnedKeySpec {
        table: "e2ee_outbox",
        key_columns: &["outbox_id"],
    },
    IdentityOwnedKeySpec {
        table: "identity_did_history",
        key_columns: &["did"],
    },
    IdentityOwnedKeySpec {
        table: "direct_e2ee_sessions",
        key_columns: &["peer_did"],
    },
    IdentityOwnedKeySpec {
        table: "direct_e2ee_signed_prekeys",
        key_columns: &["key_id"],
    },
    IdentityOwnedKeySpec {
        table: "direct_e2ee_one_time_prekeys",
        key_columns: &["key_id"],
    },
    IdentityOwnedKeySpec {
        table: "attachment_manifest_cache",
        key_columns: &["thread_kind", "thread_id", "message_id"],
    },
];

const OWNER_REQUIRED_TABLES_V17: &[&str] = &[
    "contacts",
    "contact_handle_bindings",
    "messages",
    "groups",
    "group_members",
    "relationship_events",
    "e2ee_outbox",
    "identity_did_history",
    "direct_e2ee_sessions",
    "direct_e2ee_signed_prekeys",
    "direct_e2ee_one_time_prekeys",
    "attachment_manifest_cache",
];

const OWNER_DID_SNAPSHOT_TABLES_V17: &[&str] = &[
    "conversation_summaries",
    "sync_state",
    "contacts",
    "contact_handle_bindings",
    "messages",
    "groups",
    "group_members",
    "relationship_events",
    "e2ee_outbox",
    "direct_e2ee_sessions",
    "direct_e2ee_signed_prekeys",
    "direct_e2ee_one_time_prekeys",
    "attachment_manifest_cache",
];

const IDENTITY_OWNED_INDEX_TEMPLATES: &[&str] = &[
    "CREATE INDEX IF NOT EXISTS idx_contacts_owner_identity_last_seen{s} ON contacts{s}(owner_identity_id, last_seen_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_contacts_owner_identity_source_group{s} ON contacts{s}(owner_identity_id, source_group_id)",
    "CREATE UNIQUE INDEX IF NOT EXISTS idx_contact_handle_bindings_owner_identity_handle_current_unique{s} ON contact_handle_bindings{s}(owner_identity_id, handle) WHERE is_current = 1",
    "CREATE INDEX IF NOT EXISTS idx_contact_handle_bindings_owner_identity_did{s} ON contact_handle_bindings{s}(owner_identity_id, did, last_seen_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_contact_handle_bindings_owner_identity_handle{s} ON contact_handle_bindings{s}(owner_identity_id, handle, last_seen_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_messages_owner_identity_thread{s} ON messages{s}(owner_identity_id, thread_id, sent_at)",
    "CREATE INDEX IF NOT EXISTS idx_messages_owner_identity_thread_seq{s} ON messages{s}(owner_identity_id, thread_id, server_seq)",
    "CREATE INDEX IF NOT EXISTS idx_messages_owner_identity_direction{s} ON messages{s}(owner_identity_id, direction)",
    "CREATE INDEX IF NOT EXISTS idx_messages_owner_identity_sender{s} ON messages{s}(owner_identity_id, sender_did)",
    "CREATE INDEX IF NOT EXISTS idx_messages_owner_identity_conversation{s} ON messages{s}(owner_identity_id, conversation_id, sent_at)",
    "CREATE INDEX IF NOT EXISTS idx_messages_owner_identity_conversation_expr_last{s} ON messages{s}(owner_identity_id, COALESCE(NULLIF(conversation_id, ''), thread_id), COALESCE(NULLIF(sent_at, ''), stored_at) DESC, msg_id DESC)",
    "CREATE INDEX IF NOT EXISTS idx_e2ee_outbox_owner_identity_status{s} ON e2ee_outbox{s}(owner_identity_id, local_status, updated_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_e2ee_outbox_owner_identity_sent_msg{s} ON e2ee_outbox{s}(owner_identity_id, sent_msg_id)",
    "CREATE INDEX IF NOT EXISTS idx_groups_owner_identity_status_last_message{s} ON groups{s}(owner_identity_id, membership_status, last_message_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_groups_owner_identity_slug{s} ON groups{s}(owner_identity_id, slug)",
    "CREATE INDEX IF NOT EXISTS idx_group_members_owner_identity_group_role{s} ON group_members{s}(owner_identity_id, group_id, role)",
    "CREATE INDEX IF NOT EXISTS idx_group_members_owner_identity_group_status{s} ON group_members{s}(owner_identity_id, group_id, status)",
    "CREATE INDEX IF NOT EXISTS idx_relationship_events_owner_identity_target_time{s} ON relationship_events{s}(owner_identity_id, target_did, created_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_relationship_events_owner_identity_status_time{s} ON relationship_events{s}(owner_identity_id, status, created_at DESC)",
    "CREATE UNIQUE INDEX IF NOT EXISTS idx_identity_did_history_current{s} ON identity_did_history{s}(owner_identity_id) WHERE status = 'current'",
    "CREATE UNIQUE INDEX IF NOT EXISTS idx_identity_did_history_live_did_unique{s} ON identity_did_history{s}(did) WHERE status = 'current'",
    "CREATE INDEX IF NOT EXISTS idx_direct_e2ee_sessions_owner_updated{s} ON direct_e2ee_sessions{s}(owner_identity_id, updated_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_direct_e2ee_signed_prekeys_owner_status{s} ON direct_e2ee_signed_prekeys{s}(owner_identity_id, status, updated_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_direct_e2ee_one_time_prekeys_owner_status{s} ON direct_e2ee_one_time_prekeys{s}(owner_identity_id, status, created_at ASC)",
    "CREATE INDEX IF NOT EXISTS idx_attachment_manifest_cache_owner_thread{s} ON attachment_manifest_cache{s}(owner_identity_id, thread_kind, thread_id, message_id)",
];

const INDEX_STATEMENTS: &[&str] = &[
    "CREATE INDEX IF NOT EXISTS idx_contacts_owner_identity ON contacts(owner_identity_id, last_seen_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_contacts_owner ON contacts(owner_did, last_seen_at DESC)",
    "CREATE UNIQUE INDEX IF NOT EXISTS idx_contact_handle_bindings_owner_identity_handle_current_unique ON contact_handle_bindings(owner_identity_id, handle) WHERE owner_identity_id IS NOT NULL AND is_current = 1",
    "CREATE INDEX IF NOT EXISTS idx_contact_handle_bindings_owner_identity_did ON contact_handle_bindings(owner_identity_id, did, last_seen_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_contact_handle_bindings_owner_identity_handle ON contact_handle_bindings(owner_identity_id, handle, last_seen_at DESC)",
    "CREATE UNIQUE INDEX IF NOT EXISTS idx_contact_handle_bindings_owner_handle_current_unique ON contact_handle_bindings(owner_did, handle) WHERE is_current = 1",
    "CREATE INDEX IF NOT EXISTS idx_contact_handle_bindings_owner_did ON contact_handle_bindings(owner_did, did, last_seen_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_contact_handle_bindings_owner_handle ON contact_handle_bindings(owner_did, handle, last_seen_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_messages_owner_identity_thread ON messages(owner_identity_id, thread_id, sent_at)",
    "CREATE INDEX IF NOT EXISTS idx_messages_owner_identity_thread_seq ON messages(owner_identity_id, thread_id, server_seq)",
    "CREATE INDEX IF NOT EXISTS idx_messages_owner_identity_direction ON messages(owner_identity_id, direction)",
    "CREATE INDEX IF NOT EXISTS idx_messages_owner_identity_sender ON messages(owner_identity_id, sender_did)",
    "CREATE INDEX IF NOT EXISTS idx_messages_owner_identity ON messages(owner_identity_id)",
    "CREATE INDEX IF NOT EXISTS idx_messages_owner_identity_conversation_expr_last ON messages(owner_identity_id, COALESCE(NULLIF(conversation_id, ''), thread_id), COALESCE(NULLIF(sent_at, ''), stored_at) DESC, msg_id DESC)",
    "CREATE INDEX IF NOT EXISTS idx_messages_owner_thread ON messages(owner_did, thread_id, sent_at)",
    "CREATE INDEX IF NOT EXISTS idx_messages_owner_thread_seq ON messages(owner_did, thread_id, server_seq)",
    "CREATE INDEX IF NOT EXISTS idx_messages_owner_direction ON messages(owner_did, direction)",
    "CREATE INDEX IF NOT EXISTS idx_messages_owner_sender ON messages(owner_did, sender_did)",
    "CREATE INDEX IF NOT EXISTS idx_messages_owner ON messages(owner_did)",
    "CREATE INDEX IF NOT EXISTS idx_messages_credential ON messages(credential_name)",
    "CREATE INDEX IF NOT EXISTS idx_e2ee_outbox_owner_identity_status ON e2ee_outbox(owner_identity_id, local_status, updated_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_e2ee_outbox_owner_identity_sent_msg ON e2ee_outbox(owner_identity_id, sent_msg_id)",
    "CREATE INDEX IF NOT EXISTS idx_e2ee_outbox_owner_status ON e2ee_outbox(owner_did, local_status, updated_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_e2ee_outbox_owner_sent_msg ON e2ee_outbox(owner_did, sent_msg_id)",
    "CREATE INDEX IF NOT EXISTS idx_e2ee_outbox_owner_sent_seq ON e2ee_outbox(owner_did, peer_did, sent_server_seq)",
    "CREATE INDEX IF NOT EXISTS idx_e2ee_outbox_credential ON e2ee_outbox(credential_name)",
    "CREATE INDEX IF NOT EXISTS idx_groups_owner_identity_status_last_message ON groups(owner_identity_id, membership_status, last_message_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_groups_owner_identity_slug ON groups(owner_identity_id, slug)",
    "CREATE INDEX IF NOT EXISTS idx_groups_owner_identity_updated ON groups(owner_identity_id, remote_updated_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_groups_owner_status_last_message ON groups(owner_did, membership_status, last_message_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_groups_owner_slug ON groups(owner_did, slug)",
    "CREATE INDEX IF NOT EXISTS idx_groups_owner_updated ON groups(owner_did, remote_updated_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_group_members_owner_identity_group_role ON group_members(owner_identity_id, group_id, role)",
    "CREATE INDEX IF NOT EXISTS idx_group_members_owner_identity_group_status ON group_members(owner_identity_id, group_id, status)",
    "CREATE INDEX IF NOT EXISTS idx_group_members_owner_group_role ON group_members(owner_did, group_id, role)",
    "CREATE INDEX IF NOT EXISTS idx_group_members_owner_group_status ON group_members(owner_did, group_id, status)",
    "CREATE INDEX IF NOT EXISTS idx_contacts_owner_identity_source_group ON contacts(owner_identity_id, source_group_id)",
    "CREATE INDEX IF NOT EXISTS idx_contacts_owner_source_group ON contacts(owner_did, source_group_id)",
    "CREATE INDEX IF NOT EXISTS idx_relationship_events_owner_identity_target_time ON relationship_events(owner_identity_id, target_did, created_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_relationship_events_owner_identity_status_time ON relationship_events(owner_identity_id, status, created_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_relationship_events_owner_identity_group ON relationship_events(owner_identity_id, source_group_id)",
    "CREATE INDEX IF NOT EXISTS idx_relationship_events_owner_target_time ON relationship_events(owner_did, target_did, created_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_relationship_events_owner_status_time ON relationship_events(owner_did, status, created_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_relationship_events_owner_group ON relationship_events(owner_did, source_group_id)",
    "CREATE INDEX IF NOT EXISTS idx_e2ee_sessions_owner_updated ON e2ee_sessions(owner_did, updated_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_e2ee_sessions_credential ON e2ee_sessions(credential_name)",
    "CREATE INDEX IF NOT EXISTS idx_direct_e2ee_sessions_owner_updated ON direct_e2ee_sessions(owner_identity_id, updated_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_direct_e2ee_signed_prekeys_owner_status ON direct_e2ee_signed_prekeys(owner_identity_id, status, updated_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_direct_e2ee_one_time_prekeys_owner_status ON direct_e2ee_one_time_prekeys(owner_identity_id, status, created_at ASC)",
    "CREATE INDEX IF NOT EXISTS idx_attachment_manifest_cache_owner_thread ON attachment_manifest_cache(owner_identity_id, thread_kind, thread_id, message_id)",
];

const VIEW_STATEMENTS: &[&str] = &[
    r#"CREATE VIEW IF NOT EXISTS threads AS
SELECT
    owner_identity_id,
    owner_did,
    COALESCE(NULLIF(conversation_id, ''), thread_id) AS conversation_id,
    COALESCE(NULLIF(conversation_id, ''), thread_id) AS thread_id,
    COUNT(*) AS message_count,
    SUM(CASE WHEN is_read = 0 AND direction = 0 THEN 1 ELSE 0 END) AS unread_count,
    SUM(CASE WHEN is_read = 0 AND direction = 0 AND mentions_current_user = 1 THEN 1 ELSE 0 END) AS unread_mention_count,
    (SELECT m3.msg_id FROM messages m3
     WHERE m3.owner_identity_id = m.owner_identity_id
       AND COALESCE(NULLIF(m3.conversation_id, ''), m3.thread_id) = COALESCE(NULLIF(m.conversation_id, ''), m.thread_id)
       AND m3.is_read = 0
       AND m3.direction = 0
       AND m3.mentions_current_user = 1
     ORDER BY COALESCE(m3.sent_at, m3.stored_at) ASC, m3.msg_id ASC
     LIMIT 1) AS first_unread_mention_message_id,
    MAX(COALESCE(sent_at, stored_at)) AS last_message_at,
    (SELECT m2.content FROM messages m2
     WHERE m2.owner_identity_id = m.owner_identity_id
       AND COALESCE(NULLIF(m2.conversation_id, ''), m2.thread_id) = COALESCE(NULLIF(m.conversation_id, ''), m.thread_id)
     ORDER BY COALESCE(m2.sent_at, m2.stored_at) DESC
     LIMIT 1) AS last_content
FROM messages m
GROUP BY owner_identity_id, COALESCE(NULLIF(conversation_id, ''), thread_id)"#,
    r#"CREATE VIEW IF NOT EXISTS inbox AS
SELECT * FROM messages WHERE direction = 0
    ORDER BY owner_identity_id, COALESCE(sent_at, stored_at) DESC"#,
    r#"CREATE VIEW IF NOT EXISTS outbox AS
SELECT * FROM messages WHERE direction = 1
ORDER BY owner_identity_id, COALESCE(sent_at, stored_at) DESC"#,
];

pub(crate) fn create_identity_owned_schema(
    connection: &Connection,
    mode: IdentityOwnedSchemaTableMode,
) -> crate::ImResult<()> {
    connection
        .execute_batch(&identity_owned_tables_sql(mode))
        .map_err(super::local_state_unavailable)?;
    for template in IDENTITY_OWNED_INDEX_TEMPLATES {
        connection
            .execute(&template.replace("{s}", mode.suffix()), [])
            .map_err(super::local_state_unavailable)?;
    }
    Ok(())
}

pub(crate) fn identity_owned_merge_specs() -> &'static [IdentityOwnedMergeSpec] {
    IDENTITY_OWNED_MERGE_SPECS
}

pub(crate) fn resolve_rebuild_row_owner(
    input: RebuildRowOwnershipInput,
    owner_hints: &[LocalStateOwnerHint],
) -> crate::ImResult<RebuildRowOwnerResolution> {
    if !input.owner_identity_id.trim().is_empty() {
        if let Some(owner_did) = first_non_empty([
            input.owner_did.as_str(),
            owner_hints
                .iter()
                .find(|hint| hint.owner_identity_id.trim() == input.owner_identity_id.trim())
                .map(|hint| hint.current_did.as_str())
                .unwrap_or_default(),
        ]) {
            let mut scope = crate::internal::local_state::owner_scope::OwnerScope::new(
                input.owner_identity_id,
                owner_did,
            )?;
            if !input.credential_name.trim().is_empty() {
                scope = scope.with_credential_name(input.credential_name);
            }
            return Ok(RebuildRowOwnerResolution::Resolved(scope));
        }
        return Ok(RebuildRowOwnerResolution::Unresolved(redacted_rebuild_row(
            input,
            "missing_owner_did_for_identity",
        )));
    }

    let owner_did = input.owner_did.trim();
    if owner_did.is_empty() {
        return Ok(RebuildRowOwnerResolution::Unresolved(redacted_rebuild_row(
            input,
            "missing_owner_identity_id",
        )));
    }

    let matches = owner_hints
        .iter()
        .filter(|hint| owner_hint_contains_did(hint, owner_did))
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [hint] => {
            let mut scope = crate::internal::local_state::owner_scope::OwnerScope::new(
                hint.owner_identity_id.clone(),
                hint.current_did.clone(),
            )?;
            if !input.credential_name.trim().is_empty() {
                scope = scope.with_credential_name(input.credential_name);
            }
            Ok(RebuildRowOwnerResolution::Resolved(scope))
        }
        [] => Ok(RebuildRowOwnerResolution::Unresolved(redacted_rebuild_row(
            input,
            "unresolved_owner_did",
        ))),
        _ => Ok(RebuildRowOwnerResolution::Unresolved(redacted_rebuild_row(
            input,
            "ambiguous_owner_did",
        ))),
    }
}

pub(crate) fn identity_owned_owner_invariants(
    connection: &Connection,
    mode: IdentityOwnedSchemaTableMode,
) -> crate::ImResult<Vec<OwnerInvariantViolation>> {
    let suffix = mode.suffix();
    let mut violations = Vec::new();

    for table in OWNER_REQUIRED_TABLES_V17 {
        let table_name = format!("{table}{suffix}");
        let count = count_query(
            connection,
            &format!(
                "SELECT COUNT(*) FROM {table_name} WHERE owner_identity_id IS NULL OR TRIM(owner_identity_id) = ''"
            ),
        )?;
        if count > 0 {
            violations.push(OwnerInvariantViolation {
                table,
                invariant: "owner_identity_id_required",
                row_count: count,
            });
        }
    }

    for spec in OWNER_KEY_SPECS_V17 {
        let table_name = format!("{}{suffix}", spec.table);
        let group_columns = std::iter::once("owner_identity_id")
            .chain(spec.key_columns.iter().copied())
            .collect::<Vec<_>>()
            .join(", ");
        let count = count_query(
            connection,
            &format!(
                "SELECT COUNT(*) FROM (SELECT {group_columns}, COUNT(*) AS duplicate_count FROM {table_name} GROUP BY {group_columns} HAVING COUNT(*) > 1)"
            ),
        )?;
        if count > 0 {
            violations.push(OwnerInvariantViolation {
                table: spec.table,
                invariant: "duplicate_identity_owned_key",
                row_count: count,
            });
        }
    }

    let current_per_identity = count_query(
        connection,
        &format!(
            "SELECT COUNT(*) FROM (SELECT owner_identity_id FROM identity_did_history{suffix} WHERE status = 'current' GROUP BY owner_identity_id HAVING COUNT(*) > 1)"
        ),
    )?;
    if current_per_identity > 0 {
        violations.push(OwnerInvariantViolation {
            table: "identity_did_history",
            invariant: "one_current_did_per_identity",
            row_count: current_per_identity,
        });
    }

    let duplicate_current_did = count_query(
        connection,
        &format!(
            "SELECT COUNT(*) FROM (SELECT did FROM identity_did_history{suffix} WHERE status = 'current' GROUP BY did HAVING COUNT(*) > 1)"
        ),
    )?;
    if duplicate_current_did > 0 {
        violations.push(OwnerInvariantViolation {
            table: "identity_did_history",
            invariant: "current_did_unique_across_identities",
            row_count: duplicate_current_did,
        });
    }

    let owner_did_in_conversation_id = count_query(
        connection,
        &format!(
            "SELECT COUNT(*) FROM messages{suffix} WHERE TRIM(COALESCE(conversation_id, '')) <> '' AND TRIM(COALESCE(owner_did, '')) <> '' AND instr(conversation_id, owner_did) > 0"
        ),
    )?;
    if owner_did_in_conversation_id > 0 {
        violations.push(OwnerInvariantViolation {
            table: "messages",
            invariant: "conversation_id_must_not_include_owner_did",
            row_count: owner_did_in_conversation_id,
        });
    }

    Ok(violations)
}

pub(crate) fn record_identity_did_history_transition<S: AsRef<str>>(
    connection: &mut Connection,
    owner_identity_id: &str,
    current_did: &str,
    previous_dids: &[S],
) -> crate::ImResult<BTreeMap<String, i64>> {
    let scope =
        crate::internal::local_state::owner_scope::OwnerScope::new(owner_identity_id, current_did)?;
    ensure_schema(connection)?;
    let now = now_utc_like();
    let transaction = connection
        .transaction()
        .map_err(super::local_state_unavailable)?;

    transaction
        .execute(
            r#"
UPDATE identity_did_history
SET status = 'previous',
    last_seen_at = ?1
WHERE owner_identity_id = ?2
  AND status = 'current'
  AND did <> ?3"#,
            rusqlite::params![now, scope.owner_identity_id, scope.owner_did],
        )
        .map_err(super::local_state_unavailable)?;

    for previous in normalized_previous_dids(previous_dids, &scope.owner_did) {
        transaction
            .execute(
                r#"
INSERT INTO identity_did_history
    (owner_identity_id, did, status, first_seen_at, last_seen_at)
VALUES (?1, ?2, 'previous', ?3, ?3)
ON CONFLICT(owner_identity_id, did)
DO UPDATE SET
    status = CASE
        WHEN identity_did_history.status = 'current' THEN 'current'
        ELSE 'previous'
    END,
    last_seen_at = excluded.last_seen_at"#,
                rusqlite::params![scope.owner_identity_id, previous, now],
            )
            .map_err(super::local_state_unavailable)?;
    }

    transaction
        .execute(
            r#"
INSERT INTO identity_did_history
    (owner_identity_id, did, status, first_seen_at, last_seen_at)
VALUES (?1, ?2, 'current', ?3, ?3)
ON CONFLICT(owner_identity_id, did)
DO UPDATE SET
    status = 'current',
    last_seen_at = excluded.last_seen_at"#,
            rusqlite::params![scope.owner_identity_id, scope.owner_did, now],
        )
        .map_err(super::local_state_unavailable)?;

    let mut snapshot_counts = BTreeMap::new();
    for table in OWNER_DID_SNAPSHOT_TABLES_V17 {
        let updated = transaction
            .execute(
                &format!(
                    r#"
UPDATE {table}
SET owner_did = ?1
WHERE owner_identity_id = ?2
  AND owner_did <> ?1"#
                ),
                rusqlite::params![scope.owner_did, scope.owner_identity_id],
            )
            .map_err(super::local_state_unavailable)? as i64;
        snapshot_counts.insert((*table).to_string(), updated);
    }

    transaction
        .commit()
        .map_err(super::local_state_unavailable)?;
    Ok(snapshot_counts)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnerInvariantViolation {
    pub(crate) table: &'static str,
    pub(crate) invariant: &'static str,
    pub(crate) row_count: i64,
}

pub(crate) fn ensure_schema(connection: &Connection) -> crate::ImResult<()> {
    let version = current_schema_version(connection)?;
    if version == 0 {
        create_schema(connection, true)?;
        return set_schema_version(connection, SCHEMA_VERSION);
    }
    if version > SCHEMA_VERSION {
        return Err(crate::ImError::LocalStateUnavailable {
            detail: format!(
                "sqlite schema version {version} is newer than supported {SCHEMA_VERSION}"
            ),
        });
    }
    if version == SYSTEM_NOTIFICATION_SCHEMA_VERSION - 1 {
        return migrate_v28_to_v29(connection);
    }
    if version < SCHEMA_VERSION {
        return Err(crate::ImError::LocalStateUpgradeRequired {
            from_version: version,
            target_version: SCHEMA_VERSION,
        });
    }
    create_schema(connection, false)
}

fn migrate_v28_to_v29(connection: &Connection) -> crate::ImResult<()> {
    let transaction = connection
        .unchecked_transaction()
        .map_err(super::local_state_unavailable)?;
    crate::internal::system_notification::store::create_schema(&transaction)?;
    set_schema_version(&transaction, SCHEMA_VERSION)?;
    transaction.commit().map_err(super::local_state_unavailable)
}

pub(crate) fn current_schema_version(connection: &Connection) -> crate::ImResult<i64> {
    connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(super::local_state_unavailable)
}

pub(super) fn create_schema(
    connection: &Connection,
    backfill_conversation_summaries: bool,
) -> crate::ImResult<()> {
    create_identity_owned_schema(connection, IdentityOwnedSchemaTableMode::Final)?;
    ensure_column(connection, "contacts", "peer_persona_id", "TEXT")?;
    ensure_group_member_identity_columns(connection)?;
    connection
        .execute_batch(ATTACHMENT_MANIFEST_CACHE_SQL)
        .map_err(super::local_state_unavailable)?;
    ensure_column(
        connection,
        "attachment_manifest_cache",
        "wire_message_id",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    connection
        .execute_batch(SYNC_STATE_SQL)
        .map_err(super::local_state_unavailable)?;
    connection
        .execute_batch(THREAD_READ_STATE_SQL)
        .map_err(super::local_state_unavailable)?;
    connection
        .execute_batch(MESSAGE_IDENTITY_ALIASES_SQL)
        .map_err(super::local_state_unavailable)?;
    connection
        .execute_batch(crate::internal::group_rebind_recovery::GROUP_REBIND_RECOVERY_SQL)
        .map_err(super::local_state_unavailable)?;
    connection
        .execute_batch(DIRECT_PEER_ROUTES_SQL)
        .map_err(super::local_state_unavailable)?;
    crate::internal::system_notification::store::create_schema(connection)?;
    ensure_column(connection, "direct_peer_routes", "peer_persona_id", "TEXT")?;
    ensure_column(
        connection,
        "direct_peer_routes",
        "authority_namespace",
        "TEXT",
    )?;
    let mut should_rebuild_conversation_summaries = backfill_conversation_summaries;
    if ensure_message_projection_columns(connection)? {
        backfill_message_mention_projection(connection)?;
        should_rebuild_conversation_summaries = true;
    }
    super::conversation_summaries::create_schema(connection)?;
    super::conversation_registry::create_schema(connection)?;
    super::peer_personas::create_schema(connection)?;
    super::peer_identifiers::create_schema(connection)?;
    super::peer_profiles::create_schema(connection)?;
    super::conversation_aliases::create_schema(connection)?;
    super::inbound_resolution_backlog::create_schema(connection)?;
    for view in ["threads", "inbox", "outbox"] {
        connection
            .execute(&format!("DROP VIEW IF EXISTS {view}"), [])
            .map_err(super::local_state_unavailable)?;
    }
    for statement in VIEW_STATEMENTS {
        connection
            .execute(statement, [])
            .map_err(super::local_state_unavailable)?;
    }
    if super::messages::repair_control_payload_read_projection(connection)? > 0 {
        should_rebuild_conversation_summaries = true;
    }
    if super::messages::repair_thread_read_state_alias_projection(connection)? > 0 {
        should_rebuild_conversation_summaries = true;
    }
    if super::messages::repair_group_message_identity_projection(connection)? > 0 {
        should_rebuild_conversation_summaries = true;
    }
    if should_rebuild_conversation_summaries {
        super::conversation_summaries::rebuild_all(connection)?;
    }
    if backfill_conversation_summaries
        || current_schema_version(connection)? < CONVERSATION_REGISTRY_SCHEMA_VERSION
    {
        super::conversation_registry::backfill_from_summaries(connection)?;
    }
    Ok(())
}

fn identity_owned_tables_sql(mode: IdentityOwnedSchemaTableMode) -> String {
    let suffix = mode.suffix();
    format!(
        r#"
CREATE TABLE IF NOT EXISTS contacts{suffix} (
    owner_identity_id TEXT NOT NULL,
    owner_did         TEXT NOT NULL DEFAULT '',
    did               TEXT NOT NULL,
    peer_persona_id   TEXT,
    name              TEXT,
    handle            TEXT,
    nick_name         TEXT,
    bio               TEXT,
    profile_md        TEXT,
    tags              TEXT,
    relationship      TEXT,
    source_type       TEXT,
    source_name       TEXT,
    source_group_id   TEXT,
    connected_at      TEXT,
    recommended_reason TEXT,
    followed          INTEGER NOT NULL DEFAULT 0,
    messaged          INTEGER NOT NULL DEFAULT 0,
    note              TEXT,
    first_seen_at     TEXT,
    last_seen_at      TEXT,
    metadata          TEXT,
    credential_name   TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (owner_identity_id, did)
);

CREATE TABLE IF NOT EXISTS contact_handle_bindings{suffix} (
    owner_identity_id TEXT NOT NULL,
    owner_did         TEXT NOT NULL DEFAULT '',
    handle            TEXT NOT NULL,
    did               TEXT NOT NULL,
    is_current        INTEGER NOT NULL DEFAULT 1,
    first_seen_at     TEXT NOT NULL,
    last_seen_at      TEXT NOT NULL,
    source_type       TEXT,
    source_group_id   TEXT,
    metadata          TEXT,
    credential_name   TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (owner_identity_id, handle, did)
);

CREATE TABLE IF NOT EXISTS messages{suffix} (
    msg_id            TEXT NOT NULL,
    owner_identity_id TEXT NOT NULL,
    owner_did         TEXT NOT NULL DEFAULT '',
    conversation_id   TEXT NOT NULL DEFAULT '',
    wire_thread_kind  TEXT NOT NULL DEFAULT '',
    wire_thread_ref   TEXT NOT NULL DEFAULT '',
    wire_identity_resolution_state TEXT NOT NULL DEFAULT 'legacy_unresolved',
    thread_id         TEXT NOT NULL,
    direction         INTEGER NOT NULL DEFAULT 0,
    sender_did        TEXT,
    receiver_did      TEXT,
    group_id          TEXT,
    group_did         TEXT,
    content_type      TEXT DEFAULT 'text',
    content           TEXT,
    title             TEXT,
    server_seq        INTEGER,
    sent_at           TEXT,
    stored_at         TEXT NOT NULL,
    is_e2ee           INTEGER DEFAULT 0,
    is_read           INTEGER DEFAULT 0,
    sender_name       TEXT,
    metadata          TEXT,
    mentions_current_user INTEGER NOT NULL DEFAULT 0,
    credential_name   TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (owner_identity_id, msg_id)
);

CREATE TABLE IF NOT EXISTS groups{suffix} (
    owner_identity_id TEXT NOT NULL,
    owner_did          TEXT NOT NULL DEFAULT '',
    group_id           TEXT NOT NULL,
    group_did          TEXT,
    name               TEXT,
    group_mode         TEXT NOT NULL DEFAULT 'general',
    slug               TEXT,
    description        TEXT,
    goal               TEXT,
    rules              TEXT,
    message_prompt     TEXT,
    doc_url            TEXT,
    group_owner_did    TEXT,
    group_owner_handle TEXT,
    my_role            TEXT,
    membership_status  TEXT NOT NULL DEFAULT 'active',
    join_enabled       INTEGER,
    join_code          TEXT,
    join_code_expires_at TEXT,
    member_count       INTEGER,
    last_synced_seq    INTEGER,
    last_read_seq      INTEGER,
    last_message_at    TEXT,
    remote_created_at  TEXT,
    remote_updated_at  TEXT,
    stored_at          TEXT NOT NULL,
    metadata           TEXT,
    credential_name    TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (owner_identity_id, group_id)
);

CREATE TABLE IF NOT EXISTS group_members{suffix} (
    owner_identity_id TEXT NOT NULL,
    owner_did         TEXT NOT NULL DEFAULT '',
    group_id          TEXT NOT NULL,
    user_id           TEXT NOT NULL,
    membership_id     TEXT,
    peer_persona_id   TEXT,
    member_did        TEXT,
    member_credential_did TEXT,
    member_handle     TEXT,
    anchor_kind       TEXT NOT NULL DEFAULT 'did',
    anchor_value      TEXT NOT NULL DEFAULT '',
    handle_binding_generation TEXT,
    membership_epoch TEXT,
    profile_url       TEXT,
    role              TEXT,
    status            TEXT NOT NULL DEFAULT 'active',
    joined_at         TEXT,
    sent_message_count INTEGER NOT NULL DEFAULT 0,
    last_synced_at    TEXT NOT NULL,
    metadata          TEXT,
    credential_name   TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (owner_identity_id, group_id, user_id),
    UNIQUE (owner_identity_id, group_id, membership_id)
);

CREATE TABLE IF NOT EXISTS relationship_events{suffix} (
    event_id          TEXT NOT NULL,
    owner_identity_id TEXT NOT NULL,
    owner_did         TEXT NOT NULL DEFAULT '',
    target_did        TEXT NOT NULL,
    target_handle     TEXT,
    event_type        TEXT NOT NULL,
    source_type       TEXT,
    source_name       TEXT,
    source_group_id   TEXT,
    reason            TEXT,
    score             REAL,
    status            TEXT NOT NULL DEFAULT 'pending',
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    metadata          TEXT,
    credential_name   TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (owner_identity_id, event_id)
);

CREATE TABLE IF NOT EXISTS e2ee_outbox{suffix} (
    outbox_id            TEXT NOT NULL,
    owner_identity_id    TEXT NOT NULL,
    owner_did            TEXT NOT NULL DEFAULT '',
    peer_did             TEXT NOT NULL,
    session_id           TEXT,
    original_type        TEXT NOT NULL DEFAULT 'text',
    plaintext            TEXT NOT NULL,
    local_status         TEXT NOT NULL DEFAULT 'queued',
    attempt_count        INTEGER NOT NULL DEFAULT 0,
    sent_msg_id          TEXT,
    sent_server_seq      INTEGER,
    last_error_code      TEXT,
    retry_hint           TEXT,
    failed_msg_id        TEXT,
    failed_server_seq    INTEGER,
    metadata             TEXT,
    last_attempt_at      TEXT,
    created_at           TEXT NOT NULL,
    updated_at           TEXT NOT NULL,
    credential_name      TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (owner_identity_id, outbox_id)
);

CREATE TABLE IF NOT EXISTS identity_did_history{suffix} (
    owner_identity_id TEXT NOT NULL,
    did               TEXT NOT NULL,
    status            TEXT NOT NULL DEFAULT 'current',
    first_seen_at     TEXT NOT NULL,
    last_seen_at      TEXT NOT NULL,
    metadata          TEXT,
    PRIMARY KEY (owner_identity_id, did)
);

CREATE TABLE IF NOT EXISTS direct_e2ee_sessions{suffix} (
    owner_identity_id TEXT NOT NULL,
    owner_did         TEXT NOT NULL DEFAULT '',
    peer_did          TEXT NOT NULL,
    session_id        TEXT NOT NULL,
    state_blob        BLOB NOT NULL,
    metadata_json     TEXT,
    revision          INTEGER NOT NULL DEFAULT 0,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    PRIMARY KEY (owner_identity_id, peer_did),
    UNIQUE (owner_identity_id, session_id)
);

CREATE TABLE IF NOT EXISTS direct_e2ee_signed_prekeys{suffix} (
    owner_identity_id TEXT NOT NULL,
    owner_did         TEXT NOT NULL DEFAULT '',
    key_id            TEXT NOT NULL,
    private_key_blob  BLOB NOT NULL,
    public_key_blob   BLOB,
    status            TEXT NOT NULL DEFAULT 'active',
    metadata_json     TEXT,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    PRIMARY KEY (owner_identity_id, key_id)
);

CREATE TABLE IF NOT EXISTS direct_e2ee_one_time_prekeys{suffix} (
    owner_identity_id TEXT NOT NULL,
    owner_did         TEXT NOT NULL DEFAULT '',
    key_id            TEXT NOT NULL,
    private_key_blob  BLOB NOT NULL,
    public_key_blob   BLOB,
    status            TEXT NOT NULL DEFAULT 'available',
    metadata_json     TEXT,
    created_at        TEXT NOT NULL,
    consumed_at       TEXT,
    PRIMARY KEY (owner_identity_id, key_id)
);

CREATE TABLE IF NOT EXISTS attachment_manifest_cache{suffix} (
    owner_identity_id       TEXT NOT NULL,
    owner_did               TEXT NOT NULL DEFAULT '',
    thread_kind             TEXT NOT NULL,
    thread_id               TEXT NOT NULL,
    message_id              TEXT NOT NULL,
    wire_message_id         TEXT NOT NULL DEFAULT '',
    sender_did              TEXT,
    message_security_profile TEXT NOT NULL DEFAULT 'transport-protected',
    content                 TEXT NOT NULL,
    stored_at               TEXT NOT NULL,
    PRIMARY KEY (owner_identity_id, thread_kind, thread_id, message_id)
);
"#
    )
}

fn redacted_rebuild_row(
    input: RebuildRowOwnershipInput,
    reason: &'static str,
) -> RedactedRebuildRow {
    RedactedRebuildRow {
        table: input.table,
        row_key: input.row_key,
        reason,
    }
}

fn owner_hint_contains_did(hint: &LocalStateOwnerHint, did: &str) -> bool {
    let did = did.trim();
    hint.current_did.trim() == did || hint.historical_dids.iter().any(|known| known.trim() == did)
}

fn first_non_empty<const N: usize>(values: [&str; N]) -> Option<&str> {
    values.into_iter().find(|value| !value.trim().is_empty())
}

fn normalized_previous_dids<S: AsRef<str>>(values: &[S], current_did: &str) -> Vec<String> {
    let current_did = current_did.trim();
    let mut normalized = Vec::new();
    for value in values {
        let value = value.as_ref().trim();
        if value.is_empty() || value == current_did {
            continue;
        }
        if !normalized.iter().any(|known| known == value) {
            normalized.push(value.to_owned());
        }
    }
    normalized
}

fn count_query(connection: &Connection, sql: &str) -> crate::ImResult<i64> {
    connection
        .query_row(sql, [], |row| row.get(0))
        .map_err(super::local_state_unavailable)
}

fn ensure_owner_identity_columns(connection: &Connection) -> crate::ImResult<()> {
    for table in [
        "contacts",
        "contact_handle_bindings",
        "messages",
        "e2ee_outbox",
        "groups",
        "group_members",
        "relationship_events",
    ] {
        ensure_column(connection, table, "owner_identity_id", "TEXT")?;
    }
    ensure_column(
        connection,
        "contacts",
        "credential_name",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(connection, "contacts", "peer_persona_id", "TEXT")?;
    Ok(())
}

fn ensure_direct_e2ee_session_columns(connection: &Connection) -> crate::ImResult<()> {
    ensure_column(
        connection,
        "direct_e2ee_sessions",
        "revision",
        "INTEGER NOT NULL DEFAULT 0",
    )
}

fn ensure_message_projection_columns(connection: &Connection) -> crate::ImResult<bool> {
    let mut changed = false;
    for (column, definition) in [
        ("wire_thread_kind", "TEXT NOT NULL DEFAULT ''"),
        ("wire_thread_ref", "TEXT NOT NULL DEFAULT ''"),
        (
            "wire_identity_resolution_state",
            "TEXT NOT NULL DEFAULT 'legacy_unresolved'",
        ),
    ] {
        if !has_column(connection, "messages", column)? {
            ensure_column(connection, "messages", column, definition)?;
            changed = true;
        }
    }
    if !has_column(connection, "messages", "mentions_current_user")? {
        ensure_column(
            connection,
            "messages",
            "mentions_current_user",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        changed = true;
    }
    Ok(changed)
}

fn ensure_group_member_identity_columns(connection: &Connection) -> crate::ImResult<()> {
    ensure_column(connection, "group_members", "membership_id", "TEXT")?;
    ensure_column(connection, "group_members", "peer_persona_id", "TEXT")?;
    ensure_column(connection, "group_members", "member_credential_did", "TEXT")?;
    ensure_column(
        connection,
        "group_members",
        "anchor_kind",
        "TEXT NOT NULL DEFAULT 'did'",
    )?;
    ensure_column(
        connection,
        "group_members",
        "anchor_value",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        connection,
        "group_members",
        "handle_binding_generation",
        "TEXT",
    )?;
    ensure_column(connection, "group_members", "membership_epoch", "TEXT")?;
    connection
        .execute(
            r#"
UPDATE group_members
SET anchor_kind = 'did', anchor_value = COALESCE(member_did, user_id)
WHERE TRIM(COALESCE(anchor_value, '')) = ''"#,
            [],
        )
        .map_err(super::local_state_unavailable)?;
    let rows = {
        let mut statement = connection
            .prepare(
                r#"SELECT owner_identity_id, group_id, anchor_kind, anchor_value,
                          COALESCE(membership_epoch, ''), COALESCE(membership_id, '')
FROM group_members"#,
            )
            .map_err(super::local_state_unavailable)?;
        let mapped = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })
            .map_err(super::local_state_unavailable)?;
        let mut rows = Vec::new();
        for row in mapped {
            rows.push(row.map_err(super::local_state_unavailable)?);
        }
        rows
    };
    for (owner, group, kind, anchor, epoch, membership_id) in rows {
        if membership_id.trim().is_empty() {
            connection
                .execute(
                    r#"UPDATE group_members SET membership_id = ?1,
                           member_credential_did = COALESCE(member_credential_did, member_did)
WHERE owner_identity_id = ?2 AND group_id = ?3
  AND anchor_kind = ?4 AND anchor_value = ?5"#,
                    rusqlite::params![
                        super::groups::fallback_membership_id(
                            &group,
                            &kind,
                            &anchor,
                            (!epoch.trim().is_empty()).then_some(epoch.as_str()),
                        ),
                        owner,
                        group,
                        kind,
                        anchor,
                    ],
                )
                .map_err(super::local_state_unavailable)?;
        }
    }
    connection
        .execute_batch(
            r#"CREATE UNIQUE INDEX IF NOT EXISTS idx_group_members_owner_membership
ON group_members(owner_identity_id, group_id, membership_id);"#,
        )
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

fn backfill_message_mention_projection(connection: &Connection) -> crate::ImResult<()> {
    let updates = {
        let mut statement = connection
            .prepare(
                r#"
SELECT msg_id,
       owner_identity_id,
       owner_did,
       direction,
       thread_id,
       group_id,
       group_did,
       content_type,
       content
FROM messages
WHERE mentions_current_user = 0
  AND direction = 0
  AND content_type = 'application/json'
  AND (
      TRIM(COALESCE(group_id, '')) <> ''
      OR TRIM(COALESCE(group_did, '')) <> ''
      OR thread_id LIKE 'group:%'
  )"#,
            )
            .map_err(super::local_state_unavailable)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, Option<String>>("msg_id")?.unwrap_or_default(),
                    row.get::<_, Option<String>>("owner_identity_id")?
                        .unwrap_or_default(),
                    row.get::<_, Option<String>>("owner_did")?
                        .unwrap_or_default(),
                    row.get::<_, Option<i64>>("direction")?.unwrap_or_default(),
                    row.get::<_, Option<String>>("thread_id")?
                        .unwrap_or_default(),
                    row.get::<_, Option<String>>("group_id")?
                        .unwrap_or_default(),
                    row.get::<_, Option<String>>("group_did")?
                        .unwrap_or_default(),
                    row.get::<_, Option<String>>("content_type")?
                        .unwrap_or_default(),
                    row.get::<_, Option<String>>("content")?.unwrap_or_default(),
                ))
            })
            .map_err(super::local_state_unavailable)?;
        let mut updates = Vec::new();
        for row in rows {
            let (
                msg_id,
                owner_identity_id,
                owner_did,
                direction,
                thread_id,
                group_id,
                group_did,
                content_type,
                content,
            ) = row.map_err(super::local_state_unavailable)?;
            if crate::internal::local_state::messages::mentions_current_user_for_projection(
                &owner_did,
                direction,
                &thread_id,
                &group_id,
                &group_did,
                &content_type,
                &content,
            ) {
                updates.push((owner_identity_id, msg_id));
            }
        }
        updates
    };
    for (owner_identity_id, msg_id) in updates {
        connection
            .execute(
                r#"
UPDATE messages
SET mentions_current_user = 1
WHERE owner_identity_id = ?1 AND msg_id = ?2"#,
                rusqlite::params![owner_identity_id, msg_id],
            )
            .map_err(super::local_state_unavailable)?;
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub(crate) struct OwnerIdentityBackfill {
    pub(crate) identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) credential_names: Vec<String>,
}

pub(crate) fn backfill_owner_identity_ids(
    connection: &Connection,
    identities: &[OwnerIdentityBackfill],
) -> crate::ImResult<usize> {
    let mut updated = 0;
    for table in [
        "contacts",
        "contact_handle_bindings",
        "messages",
        "e2ee_outbox",
        "groups",
        "group_members",
        "relationship_events",
    ] {
        for identity in identities {
            let identity_id = identity.identity_id.trim();
            if identity_id.is_empty() {
                continue;
            }
            for credential_name in identity.credential_names.iter().map(|value| value.trim()) {
                if credential_name.is_empty() {
                    continue;
                }
                updated += connection
                    .execute(
                        &format!(
                            r#"
UPDATE {table}
SET owner_identity_id = ?1
WHERE (owner_identity_id IS NULL OR TRIM(owner_identity_id) = '')
  AND TRIM(COALESCE(credential_name, '')) = ?2"#
                        ),
                        rusqlite::params![identity_id, credential_name],
                    )
                    .map_err(super::local_state_unavailable)?;
            }
        }
        for identity in identities {
            let identity_id = identity.identity_id.trim();
            let owner_did = identity.owner_did.trim();
            if identity_id.is_empty() || owner_did.is_empty() {
                continue;
            }
            updated += connection
                .execute(
                    &format!(
                        r#"
UPDATE {table}
SET owner_identity_id = ?1
WHERE (owner_identity_id IS NULL OR TRIM(owner_identity_id) = '')
  AND owner_did = ?2"#
                    ),
                    rusqlite::params![identity_id, owner_did],
                )
                .map_err(super::local_state_unavailable)?;
        }
    }
    Ok(updated)
}

fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> crate::ImResult<()> {
    if has_column(connection, table, column)? {
        return Ok(());
    }
    connection
        .execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
            [],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

fn has_column(connection: &Connection, table: &str, column: &str) -> crate::ImResult<bool> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(super::local_state_unavailable)?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(super::local_state_unavailable)?;
    for row in rows {
        if row.map_err(super::local_state_unavailable)? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn backfill_contact_handle_bindings(connection: &Connection) -> crate::ImResult<()> {
    let now = now_utc_like();
    connection
        .execute(
            r#"
INSERT INTO contact_handle_bindings
    (owner_identity_id, owner_did, handle, did, is_current, first_seen_at, last_seen_at, source_type, source_group_id, metadata, credential_name)
SELECT owner_identity_id,
       owner_did,
       handle,
       did,
       0,
       COALESCE(first_seen_at, ?1),
       COALESCE(last_seen_at, ?1),
       source_type,
       source_group_id,
       metadata,
       credential_name
FROM contacts
WHERE TRIM(COALESCE(handle, '')) <> ''
ON CONFLICT(owner_did, handle, did)
DO UPDATE SET
    owner_identity_id = COALESCE(excluded.owner_identity_id, contact_handle_bindings.owner_identity_id),
    last_seen_at = excluded.last_seen_at,
    source_type = COALESCE(excluded.source_type, contact_handle_bindings.source_type),
    source_group_id = COALESCE(excluded.source_group_id, contact_handle_bindings.source_group_id),
    metadata = COALESCE(excluded.metadata, contact_handle_bindings.metadata),
    credential_name = COALESCE(excluded.credential_name, contact_handle_bindings.credential_name)"#,
            [&now],
        )
        .map_err(super::local_state_unavailable)?;
    connection
        .execute(
            r#"
WITH ranked AS (
    SELECT owner_did,
           handle,
           did,
           ROW_NUMBER() OVER (
               PARTITION BY owner_did, handle
               ORDER BY COALESCE(last_seen_at, first_seen_at, ?1) DESC, did DESC
           ) AS row_num
    FROM contacts
    WHERE TRIM(COALESCE(handle, '')) <> ''
)
UPDATE contact_handle_bindings
SET is_current = CASE
    WHEN EXISTS (
        SELECT 1
        FROM ranked
        WHERE ranked.owner_did = contact_handle_bindings.owner_did
          AND ranked.handle = contact_handle_bindings.handle
          AND ranked.did = contact_handle_bindings.did
          AND ranked.row_num = 1
    ) THEN 1
    ELSE 0
END
WHERE EXISTS (
    SELECT 1
    FROM ranked
    WHERE ranked.owner_did = contact_handle_bindings.owner_did
      AND ranked.handle = contact_handle_bindings.handle
      AND ranked.did = contact_handle_bindings.did
)"#,
            [&now],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

pub(super) fn set_schema_version(connection: &Connection, version: i64) -> crate::ImResult<()> {
    connection
        .pragma_update(None, "user_version", version)
        .map_err(super::local_state_unavailable)
}

fn now_utc_like() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{seconds}")
}

#[cfg(test)]
mod tests;
