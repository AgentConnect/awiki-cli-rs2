use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::site::SiteDomain;

pub(crate) const CONTRACT: &str = "tenant-registration-policy-v1";
pub(crate) const MAX_REVISION: i64 = 9_007_199_254_740_990;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistrationMode {
    Open,
    Allowlist,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContactType {
    Phone,
    Email,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistrationPolicySource {
    Platform,
    Tenant,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AllowlistContact {
    pub contact_type: ContactType,
    pub value: String,
}

impl fmt::Debug for AllowlistContact {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AllowlistContact")
            .field("contact_type", &self.contact_type)
            .field("value", &"[redacted]")
            .finish()
    }
}

impl AllowlistContact {
    pub fn normalize(&mut self) -> crate::ImResult<()> {
        match self.contact_type {
            ContactType::Phone => {
                let digits = self
                    .value
                    .strip_prefix('+')
                    .ok_or_else(|| invalid("value", "Canonical E.164 phone required"))?;
                if !(7..=15).contains(&digits.len())
                    || digits.starts_with('0')
                    || !digits.bytes().all(|b| b.is_ascii_digit())
                {
                    return Err(invalid("value", "Canonical E.164 phone required"));
                }
            }
            ContactType::Email => {
                self.value = self.value.trim().to_lowercase();
                let parts: Vec<_> = self.value.split('@').collect();
                if self.value.chars().count() > 254
                    || parts.len() != 2
                    || parts.iter().any(|p| p.is_empty())
                    || self.value.chars().any(char::is_whitespace)
                    || self.value.contains('*')
                {
                    return Err(invalid("value", "Exact email address required"));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistrationPolicy {
    pub contract: String,
    pub domain: SiteDomain,
    pub source: RegistrationPolicySource,
    pub mode: RegistrationMode,
    pub revision: Option<i64>,
    pub allowed_contact_types: Vec<ContactType>,
    pub effective_methods: Vec<ContactType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_counts: Option<BTreeMap<ContactType, u64>>,
}

impl RegistrationPolicy {
    pub fn validate(&self, expected: &SiteDomain) -> crate::ImResult<()> {
        let revision_ok = match self.source {
            RegistrationPolicySource::Platform => self.revision.is_none(),
            RegistrationPolicySource::Tenant => self
                .revision
                .is_some_and(|n| (1..=MAX_REVISION).contains(&n)),
        };
        if self.contract != CONTRACT
            || self.domain != *expected
            || !revision_ok
            || self.allowed_contact_types.len() > 2
            || self.effective_methods.len() > 2
            || self
                .allowed_contact_types
                .iter()
                .collect::<BTreeSet<_>>()
                .len()
                != self.allowed_contact_types.len()
            || self.effective_methods.iter().collect::<BTreeSet<_>>().len()
                != self.effective_methods.len()
            || (self.source == RegistrationPolicySource::Tenant
                && self.allowed_contact_types.is_empty())
        {
            return Err(invalid_response());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigureRegistration {
    pub domain: SiteDomain,
    pub operation_id: String,
    pub expected_revision: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<RegistrationMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_contact_types: Option<Vec<ContactType>>,
    #[serde(default)]
    pub add_entries: Vec<AllowlistContact>,
    #[serde(default)]
    pub remove_entry_ids: Vec<String>,
}

impl ConfigureRegistration {
    pub fn normalize(&mut self) -> crate::ImResult<()> {
        validate_domain(&self.domain)?;
        validate_uuid(&self.operation_id)?;
        if !(1..=MAX_REVISION).contains(&self.expected_revision) {
            return Err(invalid(
                "expected_revision",
                "Positive safe policy revision required",
            ));
        }
        if let Some(methods) = &mut self.allowed_contact_types {
            if methods.is_empty()
                || methods.len() > 2
                || methods.iter().collect::<BTreeSet<_>>().len() != methods.len()
            {
                return Err(invalid(
                    "allowed_contact_types",
                    "Distinct phone/email methods required",
                ));
            }
            methods.sort();
        }
        if self.add_entries.len() + self.remove_entry_ids.len() > 100 {
            return Err(invalid("entries", "At most 100 changes per operation"));
        }
        for entry in &mut self.add_entries {
            entry.normalize()?;
        }
        self.add_entries
            .sort_by(|a, b| (a.contact_type, &a.value).cmp(&(b.contact_type, &b.value)));
        if self
            .add_entries
            .windows(2)
            .any(|entries| entries[0] == entries[1])
        {
            return Err(invalid("entries", "Duplicate normalized contact"));
        }
        for id in &self.remove_entry_ids {
            validate_uuid(id)?;
        }
        self.remove_entry_ids.sort();
        if self.remove_entry_ids.windows(2).any(|ids| ids[0] == ids[1]) {
            return Err(invalid("remove_entry_ids", "Duplicate entry id"));
        }
        if self.mode.is_none()
            && self.allowed_contact_types.is_none()
            && self.add_entries.is_empty()
            && self.remove_entry_ids.is_empty()
        {
            return Err(invalid("request", "At least one policy change required"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListRegistrationEntries {
    pub domain: SiteDomain,
    pub limit: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact_type: Option<ContactType>,
    pub include_values: bool,
}

impl ListRegistrationEntries {
    pub(crate) fn validate(&self) -> crate::ImResult<()> {
        validate_domain(&self.domain)?;
        if !(1..=100).contains(&self.limit) {
            return Err(invalid("limit", "Limit must be 1..100"));
        }
        if let Some(after) = &self.after {
            validate_uuid(after)?;
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllowlistEntry {
    pub entry_id: String,
    pub contact_type: ContactType,
    pub masked_value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

impl fmt::Debug for AllowlistEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AllowlistEntry")
            .field("entry_id", &self.entry_id)
            .field("contact_type", &self.contact_type)
            .field("value", &"[redacted]")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllowlistPage {
    pub domain: SiteDomain,
    pub revision: i64,
    pub entries: Vec<AllowlistEntry>,
    pub next_cursor: Option<String>,
}

impl AllowlistPage {
    pub(crate) fn validate(&self, request: &ListRegistrationEntries) -> crate::ImResult<()> {
        if self.domain != request.domain
            || !(1..=MAX_REVISION).contains(&self.revision)
            || self.entries.len() > request.limit as usize
            || (self.entries.is_empty() && self.next_cursor.is_some())
        {
            return Err(invalid_response());
        }
        let mut ids = BTreeSet::new();
        for entry in &self.entries {
            if validate_uuid(&entry.entry_id).is_err()
                || !ids.insert(&entry.entry_id)
                || entry.value.is_some() != request.include_values
            {
                return Err(invalid_response());
            }
        }
        if self
            .next_cursor
            .as_ref()
            .is_some_and(|id| validate_uuid(id).is_err() || Some(id) == request.after.as_ref())
        {
            return Err(invalid_response());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistrationOperation {
    pub domain: SiteDomain,
    pub operation_id: String,
    pub revision: i64,
    pub current_revision: i64,
    pub mode: RegistrationMode,
    pub allowed_contact_types: Vec<ContactType>,
    pub entry_counts: BTreeMap<ContactType, u64>,
    pub added: u32,
    pub removed: u32,
    pub changed: bool,
    pub replayed: bool,
}

impl RegistrationOperation {
    pub(crate) fn validate(&self, domain: &SiteDomain, operation: &str) -> crate::ImResult<()> {
        if self.domain != *domain
            || self.operation_id != operation
            || !(1..=MAX_REVISION).contains(&self.revision)
            || self.current_revision < self.revision
            || self.current_revision > MAX_REVISION
        {
            return Err(invalid_response());
        }
        Ok(())
    }
}

pub(crate) fn validate_domain(domain: &SiteDomain) -> crate::ImResult<()> {
    let value = domain.as_str();
    if value.len() > 253
        || !value.contains('.')
        || value != value.to_ascii_lowercase()
        || value.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
    {
        return Err(invalid("domain", "Canonical provider domain required"));
    }
    Ok(())
}

pub(crate) fn validate_uuid(value: &str) -> crate::ImResult<()> {
    let id = uuid::Uuid::parse_str(value).map_err(|_| invalid("id", "Canonical UUID required"))?;
    if id.is_nil() || id.to_string() != value {
        return Err(invalid("id", "Canonical nonzero UUID required"));
    }
    Ok(())
}

pub(crate) fn invalid(field: &str, message: &str) -> crate::ImError {
    crate::ImError::invalid_input(Some(field.to_owned()), message)
}

pub(crate) fn invalid_response() -> crate::ImError {
    crate::ImError::Serialization {
        detail: "Invalid domain registration response".to_owned(),
    }
}
