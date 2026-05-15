pub const MODULE_PATH: &str = "github.com/agent-network-protocol/anp/golang";
pub const MODULE_VERSION: &str = "v0.8.7";

pub use anp::authentication::{
    build_anp_message_service, create_did_wba_document, generate_auth_header,
    generate_http_signature_headers, resolve_did_document, resolve_did_document_sync,
    resolve_did_document_with_options, validate_did_document_binding, AnpMessageServiceOptions,
    AuthMode, AuthenticationError, DIDWbaAuthHeader, DidDocumentBundle, DidDocumentOptions,
    DidProfile, DidResolutionOptions, DidWbaVerifier, DidWbaVerifierConfig, HttpSignatureOptions,
};
pub use anp::direct_e2ee::{
    DirectE2eeError, DirectE2eeSession, DirectSessionState, OneTimePrekey, PrekeyBundle,
    SessionStore, SignedPrekey,
};
pub use anp::proof::{
    build_im_content_digest, build_im_signature_input, build_logical_target_uri,
    build_rfc9421_origin_signature_base, build_signed_request_object,
    canonicalize_signed_request_object, encode_im_signature, generate_did_wba_binding,
    generate_group_receipt_proof, generate_im_proof, generate_rfc9421_origin_proof,
    parse_im_signature_input, verify_did_wba_binding, verify_group_receipt_proof,
    verify_im_proof_with_document, verify_rfc9421_origin_proof, DidWbaBindingVerificationOptions,
    ImProof, ImProofGenerationOptions, ParsedImSignatureInput, Rfc9421OriginProof,
    Rfc9421OriginProofGenerationOptions, Rfc9421OriginProofVerificationOptions,
    SignedRequestObject, TargetKind,
};
pub use anp::{PrivateKeyMaterial, PublicKeyMaterial};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const AUTH_MODE_HTTP_SIGNATURES: AuthMode = AuthMode::HttpSignatures;
pub const AUTH_MODE_AUTO: AuthMode = AuthMode::Auto;
pub const DID_PROFILE_E1: DidProfile = DidProfile::E1;
pub const DID_PROFILE_K1: DidProfile = DidProfile::K1;
pub const TARGET_KIND_AGENT: TargetKind = TargetKind::Agent;
pub const TARGET_KIND_GROUP: TargetKind = TargetKind::Group;
pub const TARGET_KIND_SERVICE: TargetKind = TargetKind::Service;

#[derive(Debug, Clone)]
pub struct FileSessionStore {
    root: PathBuf,
}

impl FileSessionStore {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, DirectE2eeError> {
        let root = root.as_ref().to_path_buf();
        create_session_dir(&root)?;
        Ok(Self { root })
    }

    pub fn save_session(&mut self, session: &DirectSessionState) -> Result<(), DirectE2eeError> {
        write_session_json(&self.session_path(&session.session_id), session)
    }

    pub fn load_session(&self, session_id: &str) -> Result<DirectSessionState, DirectE2eeError> {
        let path = self.session_path(session_id);
        let raw = match fs::read(&path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(DirectE2eeError::SessionNotFound(session_id.to_string()));
            }
            Err(err) => return Err(store_io_error(err)),
        };
        serde_json::from_slice(&raw).map_err(store_json_error)
    }

    pub fn delete_session(&mut self, session_id: &str) -> Result<(), DirectE2eeError> {
        match fs::remove_file(self.session_path(session_id)) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(store_io_error(err)),
        }
    }

    pub fn find_by_peer_did(
        &self,
        peer_did: &str,
    ) -> Result<Option<DirectSessionState>, DirectE2eeError> {
        let mut entries = session_json_paths(&self.root)?;
        entries.sort();
        for path in entries {
            let raw = fs::read(&path).map_err(store_io_error)?;
            let session: DirectSessionState =
                serde_json::from_slice(&raw).map_err(store_json_error)?;
            if session.peer_did == peer_did {
                return Ok(Some(session));
            }
        }
        Ok(None)
    }

    fn session_path(&self, session_id: &str) -> PathBuf {
        self.root.join(format!("{session_id}.json"))
    }
}

impl SessionStore for FileSessionStore {
    fn save_session(&mut self, session: &DirectSessionState) -> Result<(), DirectE2eeError> {
        Self::save_session(self, session)
    }

    fn load_session(&self, session_id: &str) -> Result<DirectSessionState, DirectE2eeError> {
        Self::load_session(self, session_id)
    }

    fn delete_session(&mut self, session_id: &str) -> Result<(), DirectE2eeError> {
        Self::delete_session(self, session_id)
    }
}

fn session_json_paths(root: &Path) -> Result<Vec<PathBuf>, DirectE2eeError> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(store_io_error(err)),
    };
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(store_io_error)?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) == Some("json") {
            paths.push(path);
        }
    }
    Ok(paths)
}

fn write_session_json(path: &Path, session: &DirectSessionState) -> Result<(), DirectE2eeError> {
    let raw = serde_json::to_vec_pretty(session).map_err(store_json_error)?;
    write_private_file(path, &raw)?;
    Ok(())
}

fn store_io_error(err: std::io::Error) -> DirectE2eeError {
    DirectE2eeError::invalid_field(err.to_string())
}

fn store_json_error(err: serde_json::Error) -> DirectE2eeError {
    DirectE2eeError::invalid_field(err.to_string())
}

#[cfg(unix)]
fn create_session_dir(path: &Path) -> Result<(), DirectE2eeError> {
    use std::os::unix::fs::DirBuilderExt;

    let mut builder = fs::DirBuilder::new();
    builder.recursive(true).mode(0o755);
    builder.create(path).map_err(store_io_error)
}

#[cfg(not(unix))]
fn create_session_dir(path: &Path) -> Result<(), DirectE2eeError> {
    fs::create_dir_all(path).map_err(store_io_error)
}

#[cfg(unix)]
fn write_private_file(path: &Path, raw: &[u8]) -> Result<(), DirectE2eeError> {
    use std::os::unix::fs::OpenOptionsExt;

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(store_io_error)?;
    file.write_all(raw).map_err(store_io_error)
}

#[cfg(not(unix))]
fn write_private_file(path: &Path, raw: &[u8]) -> Result<(), DirectE2eeError> {
    fs::write(path, raw).map_err(store_io_error)
}
