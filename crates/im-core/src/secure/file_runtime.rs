use serde_json::{Map, Value};

pub type DirectSecureFileRuntimeRpc =
    dyn FnMut(&str, Map<String, Value>) -> crate::ImResult<Map<String, Value>>;

#[derive(Clone, Debug, PartialEq)]
pub struct DirectSecureFileRuntimeIdentity {
    pub owner_identity_id: String,
    pub owner_did: String,
    pub identity_name: String,
    pub identity_dir: std::path::PathBuf,
    pub signing_private_pem: String,
    pub agreement_private_pem: String,
    pub local_did_document: Value,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectSecureFileOutboxFlushScope {
    pub owner_identity_id: String,
    pub owner_did: String,
    pub credential_name: String,
    pub sqlite_path: std::path::PathBuf,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DirectSecureLocalAckInput {
    pub sender_did: String,
    pub recipient_did: String,
    pub sender_identity_dir: std::path::PathBuf,
    pub recipient: DirectSecureLocalAckRecipient,
    pub session_id: String,
    pub replied_message_id: String,
    pub ack_message_id: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DirectSecureLocalAckRecipient {
    pub identity: DirectSecureFileRuntimeIdentity,
    pub session_id: String,
}

pub struct DirectSecureFileRuntimeClient {
    inner: crate::internal::secure_direct::file_runtime::DirectSecureFileRuntimeClient,
}

impl DirectSecureFileRuntimeClient {
    pub fn prepare_prekey_bundle(&mut self) -> crate::ImResult<()> {
        self.inner.publish_prekey_bundle().map(|_| ())
    }

    pub fn send_text(
        &mut self,
        peer_did: &str,
        text: &str,
        operation_id: &str,
        message_id: &str,
    ) -> crate::ImResult<Map<String, Value>> {
        self.inner
            .send_text(peer_did, text, operation_id, message_id)
    }

    pub fn send_json(
        &mut self,
        peer_did: &str,
        payload: Map<String, Value>,
        operation_id: &str,
        message_id: &str,
    ) -> crate::ImResult<Map<String, Value>> {
        self.inner
            .send_json(peer_did, payload, operation_id, message_id)
    }

    pub fn process_incoming(
        &mut self,
        message: Map<String, Value>,
    ) -> crate::ImResult<Map<String, Value>> {
        self.inner.process_incoming(message)
    }

    pub fn current_session_id(&self, peer_did: &str) -> String {
        self.inner.current_session_id(peer_did)
    }
}

pub fn new_direct_secure_file_runtime_client(
    identity: DirectSecureFileRuntimeIdentity,
    rpc: Box<DirectSecureFileRuntimeRpc>,
    resolver: Box<dyn FnMut(&str) -> crate::ImResult<Value>>,
) -> crate::ImResult<DirectSecureFileRuntimeClient> {
    Ok(DirectSecureFileRuntimeClient {
        inner: crate::internal::secure_direct::file_runtime::DirectSecureFileRuntimeClient::new(
            crate::internal::secure_direct::file_runtime::DirectSecureFileRuntimeIdentity {
                owner_identity_id: identity.owner_identity_id,
                owner_did: identity.owner_did,
                identity_name: identity.identity_name,
                identity_dir: identity.identity_dir,
                signing_private_pem: identity.signing_private_pem,
                agreement_private_pem: identity.agreement_private_pem,
                local_did_document: identity.local_did_document,
            },
            rpc,
            resolver,
        )?,
    })
}

pub fn flush_direct_secure_file_outbox(
    scope: &DirectSecureFileOutboxFlushScope,
    peer_filter_did: &str,
    client: &mut DirectSecureFileRuntimeClient,
) -> Vec<String> {
    crate::internal::secure_direct::file_runtime::flush_direct_secure_file_outbox(
        &crate::internal::secure_direct::file_runtime::DirectSecureFileOutboxFlushScope {
            owner_identity_id: scope.owner_identity_id.clone(),
            owner_did: scope.owner_did.clone(),
            credential_name: scope.credential_name.clone(),
            sqlite_path: scope.sqlite_path.clone(),
        },
        peer_filter_did,
        &mut client.inner,
    )
}

pub fn encrypt_direct_secure_file_ack(input: &DirectSecureLocalAckInput) -> crate::ImResult<Value> {
    crate::internal::secure_direct::file_runtime::encrypt_direct_secure_file_ack(
        &crate::internal::secure_direct::file_runtime::DirectSecureLocalAckInput {
            sender_did: input.sender_did.clone(),
            recipient_did: input.recipient_did.clone(),
            sender_identity_dir: input.sender_identity_dir.clone(),
            recipient: crate::internal::secure_direct::file_runtime::DirectSecureLocalAckRecipient {
                identity: crate::internal::secure_direct::file_runtime::DirectSecureFileRuntimeIdentity {
                    owner_identity_id: input.recipient.identity.owner_identity_id.clone(),
                    owner_did: input.recipient.identity.owner_did.clone(),
                    identity_name: input.recipient.identity.identity_name.clone(),
                    identity_dir: input.recipient.identity.identity_dir.clone(),
                    signing_private_pem: input.recipient.identity.signing_private_pem.clone(),
                    agreement_private_pem: input.recipient.identity.agreement_private_pem.clone(),
                    local_did_document: input.recipient.identity.local_did_document.clone(),
                },
                session_id: input.recipient.session_id.clone(),
            },
            session_id: input.session_id.clone(),
            replied_message_id: input.replied_message_id.clone(),
            ack_message_id: input.ack_message_id.clone(),
        },
    )
}
