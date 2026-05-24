use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

use crate::internal::auth::session::SessionProvider;
use crate::internal::transport::{
    AttachmentObjectTransport, AuthenticatedRpcTransport, RawJsonTransport,
};

const MESSAGE_RPC_ENDPOINT: &str = crate::internal::message_runtime::read::MESSAGE_RPC_ENDPOINT;
const ATTACHMENT_DOWNLOAD_LOOKUP_PAGE_SIZE: i64 = 100;

pub(crate) struct AttachmentDownloadRuntime<'a, P, T> {
    client: &'a crate::core::ImClient,
    session_provider: P,
    transport: T,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AttachmentDownloadInput {
    pub request: crate::attachments::DownloadAttachmentRequest,
    pub resolved_peer_did: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AttachmentDownloadResult {
    pub sdk_result: crate::attachments::DownloadedAttachment,
    pub selection: crate::attachments::selection::AttachmentSelection,
    pub ticket: crate::internal::wire::attachment::AttachmentDownloadTicketResult,
}

impl<'a, P, T> AttachmentDownloadRuntime<'a, P, T>
where
    P: SessionProvider,
    T: AuthenticatedRpcTransport + RawJsonTransport + AttachmentObjectTransport,
{
    pub(crate) fn new(
        client: &'a crate::core::ImClient,
        session_provider: P,
        transport: T,
    ) -> Self {
        Self {
            client,
            session_provider,
            transport,
        }
    }

    pub(crate) fn download(
        mut self,
        input: AttachmentDownloadInput,
    ) -> crate::ImResult<AttachmentDownloadResult> {
        let sink = crate::internal::blob::sink::attachment_destination_to_sink(
            input.request.destination,
            input.request.overwrite,
        )?;
        if let crate::internal::blob::sink::AttachmentSink::LocalFile { path, overwrite } = &sink {
            crate::internal::attachment_runtime::atomic_write::validate_destination(
                path, *overwrite,
            )?;
        }
        let target = download_target(&input.request.thread, input.resolved_peer_did)?;
        self.session_provider.ensure_session(auth_scope(&target))?;
        let selection = self.find_selection(
            &target,
            input.request.message_id.as_str(),
            input.request.attachment_id.as_deref().unwrap_or_default(),
        )?;
        if selection.sender_did.trim().is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("sender_did".to_string()),
                "attachment message sender_did is required",
            ));
        }
        let attachment_service = self.resolve_attachment_service(&selection.sender_did)?;
        let ticket = self.get_download_ticket(&target, &selection, &attachment_service)?;
        let object = self
            .transport
            .get_attachment_object(&selection.object_uri, &ticket.download_ticket_b64u)?;
        let filename = Some(selection.filename.clone()).filter(|value| !value.trim().is_empty());
        let mime_type = Some(selection.mime_type.clone())
            .filter(|value| !value.trim().is_empty())
            .or(object.content_type.clone());
        let size_bytes = selection
            .size
            .trim()
            .parse()
            .ok()
            .or(Some(object.body.len() as u64));
        let destination = match sink {
            crate::internal::blob::sink::AttachmentSink::Memory => {
                crate::attachments::DownloadedAttachmentDestination::Memory(object.body)
            }
            crate::internal::blob::sink::AttachmentSink::LocalFile { path, overwrite } => {
                let path = crate::internal::attachment_runtime::atomic_write::write_bytes_atomic(
                    &path,
                    &object.body,
                    overwrite,
                )?;
                crate::attachments::DownloadedAttachmentDestination::LocalFile(path)
            }
        };
        let sdk_result = crate::attachments::DownloadedAttachment {
            attachment_id: selection.attachment_id.clone(),
            filename,
            mime_type,
            size_bytes,
            destination,
            warnings: Vec::new(),
        };
        Ok(AttachmentDownloadResult {
            sdk_result,
            selection,
            ticket,
        })
    }

    fn find_selection(
        &mut self,
        target: &DownloadTarget,
        requested_message_id: &str,
        requested_attachment_id: &str,
    ) -> crate::ImResult<crate::attachments::selection::AttachmentSelection> {
        crate::attachments::selection::find_attachment_selection_with_paging(
            |skip| self.fetch_page(target, skip),
            requested_message_id,
            requested_attachment_id,
        )
    }

    fn fetch_page(
        &mut self,
        target: &DownloadTarget,
        skip: i64,
    ) -> crate::ImResult<(Vec<Value>, bool)> {
        match target {
            DownloadTarget::Direct { peer_did } => {
                let params = crate::internal::wire::history::build_history_rpc_params(
                    &crate::internal::wire::common::WireIdentity {
                        did: self.client.did().as_str().to_string(),
                    },
                    crate::internal::wire::history::HistoryWireRequest {
                        peer_did: peer_did.clone(),
                        limit: ATTACHMENT_DOWNLOAD_LOOKUP_PAGE_SIZE,
                        cursor: None,
                        skip,
                    },
                )?;
                let raw = self.transport.authenticated_rpc(
                    MESSAGE_RPC_ENDPOINT,
                    "direct.get_history",
                    params,
                )?;
                Ok((
                    values_from_array(raw.get("messages")),
                    bool_from_value(raw.get("has_more")),
                ))
            }
            DownloadTarget::Group { group } => {
                let params = crate::internal::wire::group::build_group_messages_rpc_params(
                    self.client.did().as_str(),
                    group.as_str(),
                    ATTACHMENT_DOWNLOAD_LOOKUP_PAGE_SIZE,
                    None,
                    skip,
                )?;
                let raw = self.transport.authenticated_rpc(
                    MESSAGE_RPC_ENDPOINT,
                    "group.list_messages",
                    params,
                )?;
                Ok((
                    values_from_array(raw.get("messages")),
                    bool_from_value(raw.get("has_more")),
                ))
            }
        }
    }

    fn resolve_attachment_service(
        &mut self,
        sender_did: &str,
    ) -> crate::ImResult<crate::internal::discovery::attachment::DiscoveredAttachmentService> {
        let document = match crate::internal::discovery::did_document::resolve_did_document(
            &mut self.transport,
            sender_did,
        ) {
            Ok(document) => document,
            Err(remote_error) => {
                local_identity_document(self.client, sender_did)?.ok_or(remote_error)?
            }
        };
        crate::internal::discovery::attachment::select_attachment_rpc_service_from_document(
            sender_did, &document,
        )
    }

    fn get_download_ticket(
        &mut self,
        target: &DownloadTarget,
        selection: &crate::attachments::selection::AttachmentSelection,
        attachment_service: &crate::internal::discovery::attachment::DiscoveredAttachmentService,
    ) -> crate::ImResult<crate::internal::wire::attachment::AttachmentDownloadTicketResult> {
        let group_did = match target {
            DownloadTarget::Direct { .. } => "",
            DownloadTarget::Group { group } => group.as_str(),
        };
        let params =
            crate::internal::wire::attachment::build_attachment_download_ticket_rpc_params(
                self.client.did().as_str(),
                &attachment_service.service_did,
                &selection.sender_did,
                &selection.message_id,
                group_did,
                selection,
            )?;
        let raw = self.transport.authenticated_rpc(
            attachment_service.rpc_endpoint.as_str(),
            "attachment.get_download_ticket",
            params,
        )?;
        serde_json::from_value(raw).map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })
    }
}

fn local_identity_document(
    client: &crate::core::ImClient,
    sender_did: &str,
) -> crate::ImResult<Option<Value>> {
    let sender_did = sender_did.trim();
    if sender_did.is_empty() {
        return Ok(None);
    }
    let paths = &client.core_inner().sdk_paths().identities;
    let identities = local_registry_identities(paths)?;
    for identity in identities {
        if identity.did != sender_did {
            continue;
        }
        let identity_dir = paths.identity_root_dir.join(identity.dir_name);
        let document_path = first_existing_path(&identity_dir, &["did.json", "did_document.json"]);
        let raw = match std::fs::read(&document_path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                return Err(crate::ImError::CredentialFileUnreadable {
                    path_kind: "did_document".to_string(),
                    detail: err.to_string(),
                });
            }
        };
        return serde_json::from_slice(&raw).map(Some).map_err(|err| {
            crate::ImError::Serialization {
                detail: err.to_string(),
            }
        });
    }
    Ok(None)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LocalRegistryIdentity {
    did: String,
    dir_name: String,
}

#[derive(Debug, Deserialize)]
struct SdkRegistryFile {
    #[serde(default)]
    identities: Vec<SdkIdentityRecord>,
}

#[derive(Debug, Deserialize)]
struct SdkIdentityRecord {
    #[serde(default)]
    id: String,
    #[serde(default)]
    did: String,
    #[serde(default)]
    dir_name: Option<String>,
    #[serde(default)]
    local_alias: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LegacyRegistryFile {
    #[serde(default)]
    credentials: BTreeMap<String, LegacyIdentityRecord>,
}

#[derive(Debug, Deserialize)]
struct LegacyIdentityRecord {
    #[serde(default)]
    credential_name: String,
    #[serde(default)]
    dir_name: String,
    #[serde(default)]
    did: String,
    #[serde(default)]
    unique_id: String,
}

fn local_registry_identities(
    paths: &crate::paths::IdentityRegistryPaths,
) -> crate::ImResult<Vec<LocalRegistryIdentity>> {
    let raw = match std::fs::read(&paths.registry_path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(crate::ImError::CredentialFileUnreadable {
                path_kind: "identity_registry".to_string(),
                detail: err.to_string(),
            });
        }
    };
    if let Ok(file) = serde_json::from_slice::<SdkRegistryFile>(&raw) {
        if !file.identities.is_empty() {
            return Ok(file
                .identities
                .into_iter()
                .filter_map(|record| {
                    let did = record.did.trim().to_string();
                    if did.is_empty() {
                        return None;
                    }
                    let dir_name = first_non_empty([
                        record.dir_name.as_deref(),
                        record.local_alias.as_deref(),
                        Some(record.id.as_str()),
                    ])?
                    .to_string();
                    Some(LocalRegistryIdentity { did, dir_name })
                })
                .collect());
        }
    }
    let file: LegacyRegistryFile =
        serde_json::from_slice(&raw).map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })?;
    Ok(file
        .credentials
        .into_iter()
        .filter_map(|(alias, record)| {
            let did = record.did.trim().to_string();
            if did.is_empty() {
                return None;
            }
            let dir_name = first_non_empty([
                Some(record.dir_name.as_str()),
                Some(record.unique_id.as_str()),
                Some(record.credential_name.as_str()),
                Some(alias.as_str()),
            ])?
            .to_string();
            Some(LocalRegistryIdentity { did, dir_name })
        })
        .collect())
}

fn first_non_empty<'a>(values: impl IntoIterator<Item = Option<&'a str>>) -> Option<&'a str> {
    values
        .into_iter()
        .flatten()
        .find(|value| !value.trim().is_empty())
}

fn first_existing_path(identity_dir: &Path, names: &[&str]) -> std::path::PathBuf {
    names
        .iter()
        .map(|name| identity_dir.join(name))
        .find(|path| path.exists())
        .unwrap_or_else(|| identity_dir.join(names[0]))
}

fn download_target(
    thread: &crate::messages::ThreadRef,
    resolved_peer_did: Option<String>,
) -> crate::ImResult<DownloadTarget> {
    match thread {
        crate::messages::ThreadRef::Direct(peer) => {
            let resolved = resolved_peer_did
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| peer.as_str().trim());
            if !resolved.starts_with("did:") {
                return Err(crate::ImError::PeerNotFound {
                    peer: peer.as_str().to_string(),
                });
            }
            Ok(DownloadTarget::Direct {
                peer_did: resolved.to_string(),
            })
        }
        crate::messages::ThreadRef::Group(group) => Ok(DownloadTarget::Group {
            group: group.clone(),
        }),
        crate::messages::ThreadRef::Thread(_) => {
            Err(crate::ImError::unsupported("thread-attachment-download"))
        }
    }
}

fn auth_scope(target: &DownloadTarget) -> crate::auth::AuthScope {
    match target {
        DownloadTarget::Direct { .. } => crate::auth::AuthScope::Messaging,
        DownloadTarget::Group { .. } => crate::auth::AuthScope::GroupMessaging,
    }
}

fn values_from_array(value: Option<&Value>) -> Vec<Value> {
    value.and_then(Value::as_array).cloned().unwrap_or_default()
}

fn bool_from_value(value: Option<&Value>) -> bool {
    matches!(value, Some(Value::Bool(true)))
}

#[derive(Debug, Clone, PartialEq)]
enum DownloadTarget {
    Direct { peer_did: String },
    Group { group: crate::ids::GroupRef },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal::transport::{
        AttachmentObjectResponse, AttachmentObjectTransport, AuthenticatedRpcTransport,
        RawJsonTransport,
    };
    use serde_json::{json, Value};
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;
    use std::rc::Rc;

    #[test]
    fn attachments_download_runtime_memory_fetches_ticket_and_bytes() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let sessions = Rc::new(RefCell::new(Vec::new()));

        let result = AttachmentDownloadRuntime::new(
            &client,
            ReadySessionProvider {
                scopes: Rc::clone(&sessions),
            },
            RecordingTransport {
                calls: Rc::clone(&calls),
            },
        )
        .download(AttachmentDownloadInput {
            request: crate::attachments::DownloadAttachmentRequest {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                message_id: crate::ids::MessageId::parse("msg-attachment-1").unwrap(),
                attachment_id: Some("att-1".to_string()),
                destination: crate::attachments::AttachmentDestination::Memory,
                overwrite: false,
            },
            resolved_peer_did: None,
        })
        .unwrap();

        assert_eq!(
            sessions.borrow().as_slice(),
            &[crate::auth::AuthScope::Messaging]
        );
        assert_eq!(result.selection.attachment_id, "att-1");
        assert_eq!(result.ticket.download_ticket_b64u, "ticket-1");
        assert_eq!(result.sdk_result.attachment_id, "att-1");
        assert_eq!(result.sdk_result.filename.as_deref(), Some("report.txt"));
        assert_eq!(result.sdk_result.mime_type.as_deref(), Some("text/plain"));
        assert_eq!(result.sdk_result.size_bytes, Some(16));
        assert!(matches!(
            result.sdk_result.destination,
            crate::attachments::DownloadedAttachmentDestination::Memory(bytes)
                if bytes == b"downloaded bytes".to_vec()
        ));

        let calls = calls.borrow();
        assert_eq!(calls.len(), 4);
        let history = calls[0].rpc("direct.get_history");
        assert_eq!(history.endpoint, MESSAGE_RPC_ENDPOINT);
        assert_eq!(history.params["body"]["peer_did"], "did:example:bob");
        assert_eq!(
            history.params["body"]["limit"],
            ATTACHMENT_DOWNLOAD_LOOKUP_PAGE_SIZE
        );
        let did_doc = calls[1].get_json("https://example.com/bob/did.json");
        assert_eq!(
            did_doc.headers.get("Accept").map(String::as_str),
            Some("application/json")
        );
        let ticket = calls[2].rpc("attachment.get_download_ticket");
        assert_eq!(ticket.endpoint, "https://attachment.example/rpc");
        assert_eq!(
            ticket.params["meta"]["target"],
            json!({"kind": "service", "did": "did:web:attachment.example"})
        );
        assert_eq!(ticket.params["body"]["attachment_id"], "att-1");
        assert_eq!(
            ticket.params["body"]["object_uri"],
            "https://objects.example/att-1"
        );
        assert_eq!(
            ticket.params["body"]["sender_did"],
            "did:web:example.com:bob"
        );
        assert_eq!(ticket.params["body"]["requester_did"], "did:example:alice");
        assert_eq!(
            ticket.params["body"]["message_target_did"],
            "did:example:alice"
        );
        let object = calls[3].object_get("https://objects.example/att-1");
        assert_eq!(object.ticket, "ticket-1");
    }

    #[test]
    fn attachments_download_runtime_pages_until_selection_matches() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));

        let result = AttachmentDownloadRuntime::new(
            &client,
            ReadySessionProvider {
                scopes: Rc::new(RefCell::new(Vec::new())),
            },
            RecordingTransport {
                calls: Rc::clone(&calls),
            },
        )
        .download(AttachmentDownloadInput {
            request: crate::attachments::DownloadAttachmentRequest {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                message_id: crate::ids::MessageId::parse("msg-second-page").unwrap(),
                attachment_id: Some("att-2".to_string()),
                destination: crate::attachments::AttachmentDestination::Memory,
                overwrite: false,
            },
            resolved_peer_did: None,
        })
        .unwrap();

        assert_eq!(result.selection.attachment_id, "att-2");
        let calls = calls.borrow();
        let first = calls[0].rpc("direct.get_history");
        assert_eq!(first.params["body"].get("skip"), None);
        let second = calls[1].rpc("direct.get_history");
        assert_eq!(second.params["body"]["skip"], 1);
    }

    #[test]
    fn attachments_download_runtime_group_uses_group_scope_and_ticket_body() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let sessions = Rc::new(RefCell::new(Vec::new()));

        let result = AttachmentDownloadRuntime::new(
            &client,
            ReadySessionProvider {
                scopes: Rc::clone(&sessions),
            },
            RecordingTransport {
                calls: Rc::clone(&calls),
            },
        )
        .download(AttachmentDownloadInput {
            request: crate::attachments::DownloadAttachmentRequest {
                thread: crate::messages::ThreadRef::Group(
                    crate::ids::GroupRef::parse("did:example:group").unwrap(),
                ),
                message_id: crate::ids::MessageId::parse("group-msg-1").unwrap(),
                attachment_id: None,
                destination: crate::attachments::AttachmentDestination::Memory,
                overwrite: false,
            },
            resolved_peer_did: None,
        })
        .unwrap();

        assert_eq!(
            sessions.borrow().as_slice(),
            &[crate::auth::AuthScope::GroupMessaging]
        );
        assert_eq!(result.selection.attachment_id, "att-group-1");
        let calls = calls.borrow();
        let list = calls[0].rpc("group.list_messages");
        assert_eq!(list.params["body"]["group_did"], "did:example:group");
        let ticket = calls[2].rpc("attachment.get_download_ticket");
        assert_eq!(ticket.params["body"]["group_did"], "did:example:group");
        assert_eq!(ticket.params["body"].get("message_target_did"), None);
    }

    #[test]
    fn attachments_download_runtime_local_file_destination_writes_file() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let output = fixture.root.join("downloads").join("report.txt");
        fs::create_dir_all(output.parent().unwrap()).unwrap();

        let result = AttachmentDownloadRuntime::new(
            &client,
            ReadySessionProvider {
                scopes: Rc::new(RefCell::new(Vec::new())),
            },
            RecordingTransport {
                calls: Rc::clone(&calls),
            },
        )
        .download(AttachmentDownloadInput {
            request: crate::attachments::DownloadAttachmentRequest {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                message_id: crate::ids::MessageId::parse("msg-attachment-1").unwrap(),
                attachment_id: Some("att-1".to_string()),
                destination: crate::attachments::AttachmentDestination::LocalFile(output.clone()),
                overwrite: false,
            },
            resolved_peer_did: None,
        })
        .unwrap();

        assert!(matches!(
            result.sdk_result.destination,
            crate::attachments::DownloadedAttachmentDestination::LocalFile(path)
                if path == output
        ));
        assert_eq!(fs::read(&output).unwrap(), b"downloaded bytes");
        assert_no_attachment_temp_files(output.parent().unwrap());
        assert_eq!(calls.borrow().len(), 4);
    }

    #[test]
    fn attachments_download_runtime_local_file_rejects_existing_destination_without_network() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let output = fixture.root.join("downloads").join("report.txt");
        fs::create_dir_all(output.parent().unwrap()).unwrap();
        fs::write(&output, b"existing").unwrap();

        let err = AttachmentDownloadRuntime::new(
            &client,
            ReadySessionProvider {
                scopes: Rc::new(RefCell::new(Vec::new())),
            },
            RecordingTransport {
                calls: Rc::clone(&calls),
            },
        )
        .download(AttachmentDownloadInput {
            request: crate::attachments::DownloadAttachmentRequest {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                message_id: crate::ids::MessageId::parse("msg-attachment-1").unwrap(),
                attachment_id: Some("att-1".to_string()),
                destination: crate::attachments::AttachmentDestination::LocalFile(output.clone()),
                overwrite: false,
            },
            resolved_peer_did: None,
        })
        .unwrap_err();

        assert!(matches!(
            err,
            crate::ImError::InvalidInput { field: Some(field), message }
                if field == "destination" && message.contains("overwrite is false")
        ));
        assert_eq!(fs::read(&output).unwrap(), b"existing");
        assert_no_attachment_temp_files(output.parent().unwrap());
        assert!(calls.borrow().is_empty());
    }

    #[test]
    fn attachments_download_runtime_falls_back_to_local_identity_document_for_sender_service() {
        let fixture = Fixture::new();
        fixture.write_attachment_service_document(
            "sender",
            "did:web:example:alice",
            "https://local-attachment.example/rpc",
            "did:example:local-message-service",
        );
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));

        let result = AttachmentDownloadRuntime::new(
            &client,
            ReadySessionProvider {
                scopes: Rc::new(RefCell::new(Vec::new())),
            },
            LocalFallbackTransport {
                calls: Rc::clone(&calls),
            },
        )
        .download(AttachmentDownloadInput {
            request: crate::attachments::DownloadAttachmentRequest {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                message_id: crate::ids::MessageId::parse("msg-local-sender").unwrap(),
                attachment_id: Some("att-local".to_string()),
                destination: crate::attachments::AttachmentDestination::Memory,
                overwrite: false,
            },
            resolved_peer_did: None,
        })
        .unwrap();

        assert_eq!(result.selection.sender_did, "did:web:example:alice");
        assert_eq!(result.ticket.download_ticket_b64u, "ticket-local");
        assert!(matches!(
            result.sdk_result.destination,
            crate::attachments::DownloadedAttachmentDestination::Memory(bytes)
                if bytes == b"local document bytes".to_vec()
        ));

        let calls = calls.borrow();
        assert_eq!(calls.len(), 4);
        let history = calls[0].rpc("direct.get_history");
        assert_eq!(history.params["body"]["peer_did"], "did:example:bob");
        calls[1].get_json("https://example/alice/did.json");
        let ticket = calls[2].rpc("attachment.get_download_ticket");
        assert_eq!(ticket.endpoint, "https://local-attachment.example/rpc");
        assert_eq!(
            ticket.params["meta"]["target"],
            json!({"kind": "service", "did": "did:example:local-message-service"})
        );
        assert_eq!(ticket.params["body"]["sender_did"], "did:web:example:alice");
        let object = calls[3].object_get("https://objects.example/att-local");
        assert_eq!(object.ticket, "ticket-local");
    }

    #[derive(Clone)]
    struct ReadySessionProvider {
        scopes: Rc<RefCell<Vec<crate::auth::AuthScope>>>,
    }

    impl SessionProvider for ReadySessionProvider {
        fn ensure_session(
            &self,
            scope: crate::auth::AuthScope,
        ) -> crate::ImResult<crate::auth::SessionBundle> {
            self.scopes.borrow_mut().push(scope);
            Ok(crate::auth::SessionBundle {
                subject: crate::ids::Did::parse("did:example:alice")?,
                scope,
                expires_at: None,
                refreshed: false,
            })
        }

        fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
            unreachable!("attachment download runtime should not refresh through test provider")
        }

        fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
            unreachable!("attachment download runtime should not read status")
        }
    }

    struct RecordingTransport {
        calls: Rc<RefCell<Vec<RecordedCall>>>,
    }

    impl AuthenticatedRpcTransport for RecordingTransport {
        fn authenticated_rpc(
            &mut self,
            endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            self.calls.borrow_mut().push(RecordedCall::Rpc {
                endpoint: endpoint.to_string(),
                method: method.to_string(),
                params: params.clone(),
            });
            match method {
                "direct.get_history" => direct_history_response(params["body"]["skip"].as_i64()),
                "group.list_messages" => Ok(group_history_response()),
                "attachment.get_download_ticket" => Ok(json!({
                    "download_ticket_b64u": "ticket-1",
                    "expires_at": "2026-05-23T01:00:00Z",
                    "ticket_binding": {
                        "attachment_id": params["body"]["attachment_id"].clone()
                    }
                })),
                _ => Err(crate::ImError::TransportUnavailable {
                    detail: format!("unexpected rpc method {method} at {endpoint}"),
                }),
            }
        }
    }

    impl RawJsonTransport for RecordingTransport {
        fn get_json_url(
            &mut self,
            url: &str,
            headers: BTreeMap<String, String>,
        ) -> crate::ImResult<Value> {
            self.calls.borrow_mut().push(RecordedCall::GetJson {
                url: url.to_string(),
                headers,
            });
            Ok(json!({
                "id": "did:web:example.com:bob",
                "service": [{
                    "id": "#attachment",
                    "type": "ANPMessageService",
                    "serviceEndpoint": "https://attachment.example/rpc",
                    "serviceDid": "did:web:attachment.example",
                    "profiles": ["anp.attachment.v1"],
                    "securityProfiles": ["transport-protected"],
                    "priority": 1
                }]
            }))
        }
    }

    impl AttachmentObjectTransport for RecordingTransport {
        fn put_attachment_object(
            &mut self,
            _upload_uri: &str,
            _headers: BTreeMap<String, String>,
            _body: Vec<u8>,
        ) -> crate::ImResult<()> {
            unreachable!("download runtime should not upload objects")
        }

        fn get_attachment_object(
            &mut self,
            object_uri: &str,
            download_ticket: &str,
        ) -> crate::ImResult<AttachmentObjectResponse> {
            self.calls.borrow_mut().push(RecordedCall::GetObject {
                object_uri: object_uri.to_string(),
                ticket: download_ticket.to_string(),
            });
            Ok(AttachmentObjectResponse {
                body: b"downloaded bytes".to_vec(),
                content_type: Some("application/octet-stream".to_string()),
            })
        }
    }

    struct LocalFallbackTransport {
        calls: Rc<RefCell<Vec<RecordedCall>>>,
    }

    impl AuthenticatedRpcTransport for LocalFallbackTransport {
        fn authenticated_rpc(
            &mut self,
            endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            self.calls.borrow_mut().push(RecordedCall::Rpc {
                endpoint: endpoint.to_string(),
                method: method.to_string(),
                params: params.clone(),
            });
            match method {
                "direct.get_history" => Ok(json!({
                    "messages": [{
                        "id": "msg-local-sender",
                        "message_id": "msg-local-sender",
                        "sender_did": "did:web:example:alice",
                        "content": {
                            "attachments": [{
                                "attachment_id": "att-local",
                                "filename": "local.txt",
                                "mime_type": "text/plain",
                                "size": "20",
                                "digest": { "alg": "sha-256", "value_b64u": "digest-local" },
                                "access_info": { "object_uri": "https://objects.example/att-local" }
                            }],
                            "primary_attachment_id": "att-local"
                        }
                    }],
                    "has_more": false
                })),
                "attachment.get_download_ticket" => Ok(json!({
                    "download_ticket_b64u": "ticket-local",
                    "expires_at": "2026-05-23T01:00:00Z",
                    "ticket_binding": {
                        "attachment_id": params["body"]["attachment_id"].clone()
                    }
                })),
                _ => Err(crate::ImError::TransportUnavailable {
                    detail: format!("unexpected rpc method {method} at {endpoint}"),
                }),
            }
        }
    }

    impl RawJsonTransport for LocalFallbackTransport {
        fn get_json_url(
            &mut self,
            url: &str,
            headers: BTreeMap<String, String>,
        ) -> crate::ImResult<Value> {
            self.calls.borrow_mut().push(RecordedCall::GetJson {
                url: url.to_string(),
                headers,
            });
            Err(crate::ImError::TransportUnavailable {
                detail: format!("forced DID document miss for {url}"),
            })
        }
    }

    impl AttachmentObjectTransport for LocalFallbackTransport {
        fn put_attachment_object(
            &mut self,
            _upload_uri: &str,
            _headers: BTreeMap<String, String>,
            _body: Vec<u8>,
        ) -> crate::ImResult<()> {
            unreachable!("download runtime should not upload objects")
        }

        fn get_attachment_object(
            &mut self,
            object_uri: &str,
            download_ticket: &str,
        ) -> crate::ImResult<AttachmentObjectResponse> {
            self.calls.borrow_mut().push(RecordedCall::GetObject {
                object_uri: object_uri.to_string(),
                ticket: download_ticket.to_string(),
            });
            Ok(AttachmentObjectResponse {
                body: b"local document bytes".to_vec(),
                content_type: Some("text/plain".to_string()),
            })
        }
    }

    #[derive(Debug, Clone)]
    enum RecordedCall {
        Rpc {
            endpoint: String,
            method: String,
            params: Value,
        },
        GetJson {
            url: String,
            headers: BTreeMap<String, String>,
        },
        GetObject {
            object_uri: String,
            ticket: String,
        },
    }

    impl RecordedCall {
        fn rpc(&self, expected_method: &str) -> RecordedRpc<'_> {
            match self {
                Self::Rpc {
                    endpoint,
                    method,
                    params,
                } => {
                    assert_eq!(method, expected_method);
                    RecordedRpc { endpoint, params }
                }
                _ => panic!("expected rpc call {expected_method}, got {self:?}"),
            }
        }

        fn get_json(&self, expected_url: &str) -> RecordedGetJson<'_> {
            match self {
                Self::GetJson { url, headers } => {
                    assert_eq!(url, expected_url);
                    RecordedGetJson { headers }
                }
                _ => panic!("expected get-json call {expected_url}, got {self:?}"),
            }
        }

        fn object_get(&self, expected_uri: &str) -> RecordedGetObject<'_> {
            match self {
                Self::GetObject { object_uri, ticket } => {
                    assert_eq!(object_uri, expected_uri);
                    RecordedGetObject { ticket }
                }
                _ => panic!("expected object GET call {expected_uri}, got {self:?}"),
            }
        }
    }

    struct RecordedRpc<'a> {
        endpoint: &'a str,
        params: &'a Value,
    }

    struct RecordedGetJson<'a> {
        headers: &'a BTreeMap<String, String>,
    }

    struct RecordedGetObject<'a> {
        ticket: &'a str,
    }

    fn direct_history_response(skip: Option<i64>) -> crate::ImResult<Value> {
        if skip.unwrap_or_default() == 0 {
            Ok(json!({
                "messages": [{
                    "id": "msg-attachment-1",
                    "message_id": "msg-attachment-1",
                    "sender_did": "did:web:example.com:bob",
                    "content": {
                        "attachments": [{
                            "attachment_id": "att-1",
                            "filename": "report.txt",
                            "mime_type": "text/plain",
                            "size": "16",
                            "digest": { "alg": "sha-256", "value_b64u": "digest-1" },
                            "access_info": { "object_uri": "https://objects.example/att-1" }
                        }],
                        "primary_attachment_id": "att-1",
                        "caption": "report"
                    }
                }],
                "has_more": true
            }))
        } else {
            Ok(json!({
                "messages": [{
                    "id": "msg-second-page",
                    "message_id": "msg-second-page",
                    "sender_did": "did:web:example.com:bob",
                    "content": {
                        "attachments": [{
                            "attachment_id": "att-2",
                            "filename": "second.txt",
                            "mime_type": "text/plain",
                            "size": "16",
                            "digest": { "alg": "sha-256", "value_b64u": "digest-2" },
                            "access_info": { "object_uri": "https://objects.example/att-2" }
                        }],
                        "primary_attachment_id": "att-2"
                    }
                }],
                "has_more": false
            }))
        }
    }

    fn group_history_response() -> Value {
        json!({
            "messages": [{
                "id": "group-msg-1",
                "message_id": "group-msg-1",
                "sender_did": "did:web:example.com:bob",
                "content": serde_json::to_string(&json!({
                    "attachments": [{
                        "attachment_id": "att-group-1",
                        "filename": "group.txt",
                        "mime_type": "text/plain",
                        "size": "16",
                        "digest": { "alg": "sha-256", "value_b64u": "digest-group" },
                        "access_info": { "object_uri": "https://objects.example/att-1" }
                    }],
                    "primary_attachment_id": "att-group-1"
                }))
                .unwrap()
            }],
            "has_more": false
        })
    }

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = unique_temp_root();
            let identities = root.join("identities");
            fs::create_dir_all(identities.join("alice")).unwrap();
            fs::write(identities.join("default"), "alice\n").unwrap();
            fs::write(
                identities.join("registry.json"),
                r#"{
                  "default_identity": "alice",
                  "identities": [{
                    "id": "alice-id",
                    "did": "did:example:alice",
                    "local_alias": "alice",
                    "ready_for_auth": true,
                    "ready_for_messaging": true,
                    "missing": []
                  }]
                }"#,
            )
            .unwrap();
            Self { root }
        }

        fn write_attachment_service_document(
            &self,
            alias: &str,
            did: &str,
            rpc_endpoint: &str,
            service_did: &str,
        ) {
            let identity_dir = self.root.join("identities").join(alias);
            fs::create_dir_all(&identity_dir).unwrap();
            fs::write(
                self.root.join("identities").join("registry.json"),
                format!(
                    r#"{{
                      "default_identity": "alice",
                      "identities": [
                        {{
                          "id": "alice-id",
                          "did": "did:example:alice",
                          "local_alias": "alice",
                          "ready_for_auth": true,
                          "ready_for_messaging": true,
                          "missing": []
                        }},
                        {{
                          "id": "{alias}-id",
                          "did": "{did}",
                          "local_alias": "{alias}",
                          "ready_for_auth": true,
                          "ready_for_messaging": true,
                          "missing": []
                        }}
                      ]
                    }}"#
                ),
            )
            .unwrap();
            fs::write(
                identity_dir.join("did.json"),
                serde_json::to_vec_pretty(&json!({
                    "id": did,
                    "service": [{
                        "id": "#attachment",
                        "type": "ANPMessageService",
                        "serviceEndpoint": rpc_endpoint,
                        "serviceDid": service_did,
                        "profiles": ["anp.attachment.v1"],
                        "securityProfiles": ["transport-protected"],
                        "priority": 1
                    }]
                }))
                .unwrap(),
            )
            .unwrap();
        }

        fn client(&self) -> crate::core::ImClient {
            crate::core::ImCore::new(
                crate::ImCoreConfig {
                    service_base_url: crate::ServiceEndpoint::parse("https://example.test")
                        .unwrap(),
                    did_domain: "awiki.test".to_string(),
                    user_service_endpoint: None,
                    message_service_endpoint: None,
                    mail_service_endpoint: None,
                    anp_service_endpoint: None,
                    anp_service_did: None,
                    transport_policy: crate::MessageTransportPolicy::HttpOnly,
                },
                crate::ImCorePaths {
                    identities: crate::paths::IdentityRegistryPaths {
                        identity_root_dir: self.root.join("identities"),
                        registry_path: self.root.join("identities").join("registry.json"),
                        default_identity_path: Some(self.root.join("identities").join("default")),
                    },
                    local_state: crate::paths::LocalStatePaths {
                        sqlite_path: self.root.join("local").join("im.sqlite"),
                    },
                    runtime: crate::paths::RuntimePaths {
                        cache_dir: self.root.join("cache"),
                        temp_dir: self.root.join("tmp"),
                    },
                },
            )
            .unwrap()
            .client(crate::identity::IdentitySelector::LocalAlias(
                "alice".to_string(),
            ))
            .unwrap()
        }
    }

    fn unique_temp_root() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "im-core-attachment-download-runtime-{}-{nanos}",
            std::process::id()
        ))
    }

    fn assert_no_attachment_temp_files(path: &std::path::Path) {
        let leftovers: Vec<_> = fs::read_dir(path)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".awiki-attachment-download-"))
            .collect();
        assert_eq!(leftovers, Vec::<String>::new());
    }
}
