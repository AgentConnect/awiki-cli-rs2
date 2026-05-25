#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenerLookupSession {
    pub identity_name: String,
    pub current_record: Option<ListenerLookupRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenerLookupSummary {
    pub identity_name: String,
    pub did: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenerLookupRecord {
    pub identity_name: String,
    pub did: String,
}

pub trait ListenerLookupManager {
    fn list_summaries(&mut self) -> Option<Vec<ListenerLookupSummary>>;
    fn load_record(&mut self, identity_name: &str) -> Option<ListenerLookupRecord>;
}

pub fn active_session_by_did<'a>(
    sessions: &'a [ListenerLookupSession],
    did: &str,
) -> Option<&'a ListenerLookupSession> {
    if did.trim().is_empty() {
        return None;
    }
    sessions.iter().find(|session| {
        session
            .current_record
            .as_ref()
            .is_some_and(|record| record.did == did)
    })
}

pub fn record_by_did(
    did: &str,
    manager: Option<&mut dyn ListenerLookupManager>,
) -> Option<ListenerLookupRecord> {
    if did.trim().is_empty() {
        return None;
    }
    let manager = manager?;
    let summaries = manager.list_summaries()?;
    for summary in summaries {
        if summary.did != did {
            continue;
        }
        return manager.load_record(&summary.identity_name);
    }
    None
}

pub fn has_runtime_session_for_did(
    sessions: &[ListenerLookupSession],
    did: &str,
    mut manager: Option<&mut dyn ListenerLookupManager>,
) -> bool {
    if did.trim().is_empty() {
        return false;
    }
    for session in sessions {
        if session
            .current_record
            .as_ref()
            .is_some_and(|record| record.did == did)
        {
            return true;
        }
        let Some(manager) = manager.as_mut() else {
            continue;
        };
        if manager
            .load_record(&session.identity_name)
            .is_some_and(|record| record.did == did)
        {
            return true;
        }
    }
    false
}
