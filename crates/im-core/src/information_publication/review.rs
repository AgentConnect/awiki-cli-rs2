use crate::{ImError, ImResult};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Snapshot {
    #[serde(rename = "type")]
    object_type: String,
    profile: String,
    operation: String,
    operation_id: String,
    target_service_id: String,
    tenant_id: String,
    information_id: String,
    version: i64,
    publication_round: i64,
    policy_revision: i64,
    publisher: Publisher,
    content: Content,
    issued_at: String,
    expires_at: String,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Publisher {
    member_id: String,
    handle: String,
    did: String,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Content {
    media_type: String,
    sha256: String,
    title: String,
    path: String,
    accept_responses: bool,
}

/// Validated immutable review. It cannot be deserialized or constructed with arbitrary fields.
/// A host must display `presentation()` and collect explicit confirmation of `intent_hash()`.
pub struct InformationPublicationReview {
    pub(super) snapshot: serde_json::Value,
    markdown: String,
    intent_hash: String,
    issued_at: i64,
    expires_at: i64,
}
impl InformationPublicationReview {
    /// `trusted_target`, `tenant_id`, and `operation_id` must come from local selection,
    /// never solely from an incoming message or the snapshot being checked.
    pub fn parse(
        snapshot_json: &[u8],
        markdown: String,
        trusted_target: &str,
        tenant_id: &str,
        operation_id: &str,
        expected_intent_hash: &str,
    ) -> ImResult<Self> {
        if snapshot_json.len() > 16 * 1024
            || markdown.len() > 1024 * 1024
            || markdown.contains('\0')
            || markdown.trim().is_empty()
        {
            return Err(invalid());
        }
        // Typed deserialization rejects duplicate and unknown fields at every nesting level.
        let s: Snapshot = serde_json::from_slice(snapshot_json).map_err(|_| invalid())?;
        let slug = s
            .content
            .path
            .strip_prefix("/pages/")
            .and_then(|s| s.strip_suffix(".md"));
        let issued_at = timestamp(&s.issued_at)?;
        let expires_at = timestamp(&s.expires_at)?;
        if s.object_type != "InformationPublication"
            || s.profile != "awiki-information-publish-v1"
            || s.operation != "information.publish"
            || s.target_service_id != trusted_target
            || !s
                .target_service_id
                .strip_prefix("urn:uuid:")
                .is_some_and(uuid)
            || s.tenant_id != tenant_id
            || s.operation_id != operation_id
            || [
                &s.tenant_id,
                &s.operation_id,
                &s.information_id,
                &s.publisher.member_id,
            ]
            .into_iter()
            .any(|id| !uuid(id))
            || [s.version, s.publication_round, s.policy_revision]
                .into_iter()
                .any(|v| !(1..=MAX_SAFE_INTEGER).contains(&v))
            || s.content.media_type != "text/markdown"
            || s.content.sha256 != hash(markdown.as_bytes())
            || s.content.title.trim().is_empty()
            || s.content.title.len() > 300
            || s.content.title.contains('\0')
            || slug.is_none_or(|s| {
                s.is_empty()
                    || s.len() > 100
                    || !s
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            })
            || !handle(&s.publisher.handle)
            || !s.publisher.did.starts_with("did:wba:")
            || s.publisher.did.len() > 2048
            || s.publisher
                .did
                .bytes()
                .any(|b| !b.is_ascii_graphic() || matches!(b, b'#' | b'?' | b'\\'))
            || !s
                .publisher
                .did
                .rsplit(':')
                .next()
                .is_some_and(|s| s.starts_with("e1_") && s.len() > 3)
            || !(1..=86400).contains(&(expires_at - issued_at))
        {
            return Err(invalid());
        }
        let snapshot = serde_json::to_value(s).map_err(|_| invalid())?;
        let intent_hash =
            hash(&serde_json_canonicalizer::to_vec(&snapshot).map_err(|_| invalid())?);
        if intent_hash != expected_intent_hash {
            return Err(invalid());
        }
        Ok(Self {
            snapshot,
            markdown,
            intent_hash,
            issued_at,
            expires_at,
        })
    }
    pub fn intent_hash(&self) -> &str {
        &self.intent_hash
    }
    /// JSON escapes terminal controls while preserving the exact signed Markdown bytes.
    pub fn presentation(&self) -> serde_json::Value {
        serde_json::json!({"snapshot":self.snapshot, "intent_hash":self.intent_hash, "markdown":self.markdown})
    }
    pub(super) fn check_time(&self, now: i64) -> ImResult<()> {
        if now >= self.expires_at || self.issued_at > now.saturating_add(30) {
            return Err(invalid());
        }
        Ok(())
    }
}
pub(super) fn invalid() -> ImError {
    ImError::invalid_input(
        Some("information_publication".to_owned()),
        "invalid or expired publication review",
    )
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn timestamp(s: &str) -> ImResult<i64> {
    let t = chrono::DateTime::parse_from_rfc3339(s).map_err(|_| invalid())?;
    if s.len() != 20 || t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true) != s {
        return Err(invalid());
    }
    Ok(t.timestamp())
}
fn uuid(s: &str) -> bool {
    s.len() == 36
        && s != "00000000-0000-0000-0000-000000000000"
        && s.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
            }
        })
}
fn handle(s: &str) -> bool {
    let Some((local, domain)) = s.split_once('.') else {
        return false;
    };
    s.len() <= 317
        && !local.is_empty()
        && local.len() <= 63
        && !local.starts_with('-')
        && !local.ends_with('-')
        && !local.contains("--")
        && local
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && domain.contains('.')
        && domain.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}
