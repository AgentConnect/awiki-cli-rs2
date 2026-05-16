use crate::anpsdk::{
    self, FileOneTimePrekeyStore, FileSessionStore, FileSignedPrekeyStore, PrivateKeyMaterial,
};
use crate::identity::{types::StoredIdentity, Manager};
use serde_json::Value;
use std::path::Path;

pub struct PreparedSecureE2EEClient {
    pub owner_did: String,
    pub signing_key_id: String,
    pub agreement_key_id: String,
    pub signing_private: PrivateKeyMaterial,
    pub agreement_private: PrivateKeyMaterial,
    pub session_store: FileSessionStore,
    pub signed_prekey_store: FileSignedPrekeyStore,
    pub one_time_prekey_store: FileOneTimePrekeyStore,
}

pub fn prepare_secure_e2ee_client_for_record(
    manager: Option<&Manager>,
    record: Option<&StoredIdentity>,
) -> Result<PreparedSecureE2EEClient, String> {
    let manager = manager.ok_or_else(|| "identity manager is required".to_string())?;
    let record = record.ok_or_else(|| "identity record is required".to_string())?;
    let paths = manager
        .paths_for_identity(&record.identity_name)
        .map_err(|err| err.to_string())?;
    let signing_private = anpsdk::PrivateKeyMaterial::from_pem(&record.key1_private_pem)
        .map_err(|err| format!("parse DID signing private key: {err}"))?;
    let agreement_private =
        anpsdk::PrivateKeyMaterial::from_pem(&record.e2ee_agreement_private_pem)
            .map_err(|err| format!("parse E2EE agreement private key: {err}"))?;
    let identity_dir = Path::new(&paths.identity_dir);
    let session_store = FileSessionStore::new(identity_dir.join("p5-e2ee-sessions"))
        .map_err(|err| err.to_string())?;
    let signed_prekey_store = FileSignedPrekeyStore::new(identity_dir.join("p5-signed-prekeys"))
        .map_err(|err| err.to_string())?;
    let one_time_prekey_store =
        FileOneTimePrekeyStore::new(identity_dir.join("p5-one-time-prekeys"))
            .map_err(|err| err.to_string())?;
    Ok(PreparedSecureE2EEClient {
        owner_did: record.did.clone(),
        signing_key_id: format!("{}#key-1", record.did),
        agreement_key_id: format!("{}#key-3", record.did),
        signing_private,
        agreement_private,
        session_store,
        signed_prekey_store,
        one_time_prekey_store,
    })
}

pub fn local_did_document(manager: Option<&Manager>, did: &str) -> Option<Value> {
    let manager = manager?;
    if did.is_empty() {
        return None;
    }
    let summaries = manager.list().ok()?;
    for summary in summaries {
        if summary.did != did {
            continue;
        }
        let record = manager.load(&summary.identity_name).ok()?;
        return record.did_document;
    }
    None
}

pub fn resolve_secure_e2ee_local_document(
    manager: Option<&Manager>,
    record: Option<&StoredIdentity>,
    did: &str,
) -> Option<Value> {
    if did.is_empty() {
        return None;
    }
    if let Some(record) = record {
        if did == record.did {
            if let Some(document) = &record.did_document {
                return Some(document.clone());
            }
        }
    }
    local_did_document(manager, did)
}
