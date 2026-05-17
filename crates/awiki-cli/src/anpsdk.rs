pub const MODULE_PATH: &str = "github.com/agent-network-protocol/anp/golang";
pub const MODULE_VERSION: &str = "v0.8.7";
#[allow(non_upper_case_globals)]
pub const ModulePath: &str = MODULE_PATH;
#[allow(non_upper_case_globals)]
pub const ModuleVersion: &str = MODULE_VERSION;

pub use anp::authentication::{
    build_anp_message_service, build_anp_message_service as BuildANPMessageService,
    create_did_wba_document, create_did_wba_document as CreateDidWBADocument, generate_auth_header,
    generate_auth_header as GenerateAuthHeader, generate_http_signature_headers,
    generate_http_signature_headers as GenerateHTTPSignatureHeaders, resolve_did_document,
    resolve_did_document as ResolveDidDocument, resolve_did_document_sync,
    resolve_did_document_with_options,
    resolve_did_document_with_options as ResolveDidDocumentWithOptions,
    validate_did_document_binding, AnpMessageServiceOptions, AuthMode, AuthenticationError,
    DIDWbaAuthHeader, DidDocumentBundle, DidDocumentOptions, DidProfile, DidResolutionOptions,
    DidWbaVerifier, DidWbaVerifierConfig, HttpSignatureOptions,
};
pub use anp::direct_e2ee::{
    build_prekey_bundle, direct_cipher_send_request, direct_init_send_request,
    extract_x25519_public_key, message_service_did_from_document, plaintext_to_value,
    prekey_bundle_get_request, prekey_bundle_publish_request, should_retry_without_opk_message,
    validate_direct_send_ids, verify_prekey_bundle, ApplicationPlaintext, DirectCipherBody,
    DirectE2eeError, DirectE2eeSession, DirectEnvelopeMetadata, DirectInitBody, DirectSessionState,
    OneTimePrekey, PendingOutboundRecord, PendingOutboundStore, PrekeyBundle, RatchetHeader,
    SessionStore, SignedPrekey, SignedPrekeyStore,
};
pub use anp::proof::{
    build_im_content_digest, build_im_content_digest as BuildIMContentDigest,
    build_im_signature_input, build_im_signature_input as BuildIMSignatureInput,
    build_logical_target_uri, build_logical_target_uri as BuildLogicalTargetURI,
    build_rfc9421_origin_signature_base,
    build_rfc9421_origin_signature_base as BuildRFC9421OriginSignatureBase,
    build_signed_request_object, build_signed_request_object as BuildSignedRequestObject,
    canonicalize_signed_request_object,
    canonicalize_signed_request_object as CanonicalizeSignedRequestObject, encode_im_signature,
    encode_im_signature as EncodeIMSignature, generate_did_wba_binding,
    generate_did_wba_binding as GenerateDidWbaBinding, generate_group_receipt_proof,
    generate_group_receipt_proof as GenerateGroupReceiptProof, generate_im_proof,
    generate_im_proof as GenerateIMProof, generate_rfc9421_origin_proof,
    generate_rfc9421_origin_proof as GenerateRFC9421OriginProof, parse_im_signature_input,
    parse_im_signature_input as ParseIMSignatureInput, verify_did_wba_binding,
    verify_did_wba_binding as VerifyDidWbaBinding, verify_group_receipt_proof,
    verify_group_receipt_proof as VerifyGroupReceiptProof, verify_im_proof_with_document,
    verify_im_proof_with_document as VerifyIMProofWithDocument, verify_rfc9421_origin_proof,
    verify_rfc9421_origin_proof as VerifyRFC9421OriginProof, DidWbaBindingVerificationOptions,
    ImProof, ImProof as IMProof, ImProofGenerationOptions,
    ImProofGenerationOptions as IMGenerationOptions, ParsedImSignatureInput,
    ParsedImSignatureInput as ParsedIMSignatureInput, Rfc9421OriginProof,
    Rfc9421OriginProof as RFC9421OriginProof, Rfc9421OriginProofGenerationOptions,
    Rfc9421OriginProofGenerationOptions as RFC9421OriginProofGenerationOptions,
    Rfc9421OriginProofVerificationOptions,
    Rfc9421OriginProofVerificationOptions as RFC9421OriginProofVerificationOptions,
    SignedRequestObject, TargetKind,
};
pub use anp::{PrivateKeyMaterial, PublicKeyMaterial};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
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

#[allow(non_upper_case_globals)]
pub const DidProfileE1: DidProfile = DID_PROFILE_E1;
#[allow(non_upper_case_globals)]
pub const DidProfileK1: DidProfile = DID_PROFILE_K1;
#[allow(non_upper_case_globals)]
pub const AuthModeHTTPSignatures: AuthMode = AUTH_MODE_HTTP_SIGNATURES;
#[allow(non_upper_case_globals)]
pub const AuthModeAuto: AuthMode = AUTH_MODE_AUTO;
#[allow(non_upper_case_globals)]
pub const TargetKindAgent: TargetKind = TARGET_KIND_AGENT;
#[allow(non_upper_case_globals)]
pub const TargetKindGroup: TargetKind = TARGET_KIND_GROUP;
#[allow(non_upper_case_globals)]
pub const TargetKindService: TargetKind = TARGET_KIND_SERVICE;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyType {
    #[serde(rename = "secp256k1")]
    Secp256k1,
    #[serde(rename = "secp256r1")]
    Secp256r1,
    #[serde(rename = "ed25519")]
    Ed25519,
    #[serde(rename = "x25519")]
    X25519,
}

impl KeyType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Secp256k1 => "secp256k1",
            Self::Secp256r1 => "secp256r1",
            Self::Ed25519 => "ed25519",
            Self::X25519 => "x25519",
        }
    }
}

impl std::fmt::Display for KeyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

pub const KEY_TYPE_SECP256R1: KeyType = KeyType::Secp256r1;
pub const KEY_TYPE_SECP256K1: KeyType = KeyType::Secp256k1;
pub const KEY_TYPE_ED25519: KeyType = KeyType::Ed25519;
pub const KEY_TYPE_X25519: KeyType = KeyType::X25519;

#[allow(non_upper_case_globals)]
pub const KeyTypeSecp256r1: KeyType = KEY_TYPE_SECP256R1;
#[allow(non_upper_case_globals)]
pub const KeyTypeSecp256k1: KeyType = KEY_TYPE_SECP256K1;
#[allow(non_upper_case_globals)]
pub const KeyTypeEd25519: KeyType = KEY_TYPE_ED25519;
#[allow(non_upper_case_globals)]
pub const KeyTypeX25519: KeyType = KEY_TYPE_X25519;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GeneratedKeyPairPEM {
    pub private_key_pem: String,
    pub public_key_pem: String,
}

pub fn generate_key_pair_pem(
    key_type: KeyType,
) -> Result<(PrivateKeyMaterial, PublicKeyMaterial, GeneratedKeyPairPEM), String> {
    let (profile, fragment) = match key_type {
        KeyType::Ed25519 => (DidProfile::E1, "key-1"),
        KeyType::Secp256k1 => (DidProfile::K1, "key-1"),
        KeyType::Secp256r1 => (DidProfile::E1, "key-2"),
        KeyType::X25519 => (DidProfile::E1, "key-3"),
    };
    let bundle = create_did_wba_document(
        "example.com",
        DidDocumentOptions::default().with_profile(profile),
    )
    .map_err(|err| err.to_string())?;
    let private_key = bundle
        .private_key_pem(fragment)
        .ok_or_else(|| format!("missing generated key: {fragment}"))
        .and_then(private_key_from_pem)?;
    let public_key = bundle
        .public_key_pem(fragment)
        .ok_or_else(|| format!("missing generated public key: {fragment}"))
        .and_then(public_key_from_pem)?;
    let pair = GeneratedKeyPairPEM {
        private_key_pem: private_key.to_pem(),
        public_key_pem: public_key.to_pem(),
    };
    Ok((private_key, public_key, pair))
}

pub fn private_key_from_pem(input: impl AsRef<str>) -> Result<PrivateKeyMaterial, String> {
    PrivateKeyMaterial::from_pem(input.as_ref()).map_err(|err| err.to_string())
}

pub fn public_key_from_pem(input: impl AsRef<str>) -> Result<PublicKeyMaterial, String> {
    PublicKeyMaterial::from_pem(input.as_ref()).map_err(|err| err.to_string())
}

#[allow(non_snake_case)]
pub fn GenerateKeyPairPEM(
    key_type: KeyType,
) -> Result<(PrivateKeyMaterial, PublicKeyMaterial, GeneratedKeyPairPEM), String> {
    generate_key_pair_pem(key_type)
}

#[allow(non_snake_case)]
pub fn PrivateKeyFromPEM(input: impl AsRef<str>) -> Result<PrivateKeyMaterial, String> {
    private_key_from_pem(input)
}

#[allow(non_snake_case)]
pub fn PublicKeyFromPEM(input: impl AsRef<str>) -> Result<PublicKeyMaterial, String> {
    public_key_from_pem(input)
}

#[allow(non_snake_case)]
pub fn NewDIDWbaAuthHeader(
    did_document_path: impl AsRef<Path>,
    private_key_path: impl AsRef<Path>,
    auth_mode: AuthMode,
) -> DIDWbaAuthHeader {
    DIDWbaAuthHeader::new(did_document_path, private_key_path, auth_mode)
}

#[allow(non_snake_case)]
pub fn NewDidWbaVerifier(config: DidWbaVerifierConfig) -> DidWbaVerifier {
    DidWbaVerifier::new(config)
}

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

#[allow(non_snake_case)]
pub fn NewFileSessionStore(root: impl AsRef<Path>) -> Result<FileSessionStore, DirectE2eeError> {
    FileSessionStore::new(root)
}

#[derive(Debug, Clone)]
pub struct FileSignedPrekeyStore {
    root: PathBuf,
}

impl FileSignedPrekeyStore {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, DirectE2eeError> {
        let root = root.as_ref().to_path_buf();
        create_store_dir(&root)?;
        Ok(Self { root })
    }

    pub fn save_signed_prekey(
        &mut self,
        key_id: &str,
        private_key: &PrivateKeyMaterial,
        metadata: &SignedPrekey,
    ) -> Result<(), DirectE2eeError> {
        write_private_file(&self.pem_path(key_id), private_key.to_pem().as_bytes())?;
        write_public_json(&self.json_path(key_id), metadata)?;
        write_public_file(&self.latest_path(), key_id.as_bytes())
    }

    pub fn load_signed_prekey(
        &self,
        key_id: &str,
    ) -> Result<(PrivateKeyMaterial, SignedPrekey), DirectE2eeError> {
        let pem_path = self.pem_path(key_id);
        let raw = match fs::read_to_string(&pem_path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(DirectE2eeError::invalid_field(format!(
                    "signed prekey not found: {key_id}"
                )));
            }
            Err(err) => return Err(store_io_error(err)),
        };
        let private_key = PrivateKeyMaterial::from_pem(&raw)
            .map_err(|err| DirectE2eeError::invalid_field(err.to_string()))?;
        let metadata = read_json_file(&self.json_path(key_id))?;
        Ok((private_key, metadata))
    }

    pub fn load_latest_signed_prekey(
        &self,
    ) -> Result<Option<(PrivateKeyMaterial, SignedPrekey)>, DirectE2eeError> {
        let raw = match fs::read_to_string(self.latest_path()) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(store_io_error(err)),
        };
        self.load_signed_prekey(raw.trim()).map(Some)
    }

    fn pem_path(&self, key_id: &str) -> PathBuf {
        self.root.join(format!("{key_id}.pem"))
    }

    fn json_path(&self, key_id: &str) -> PathBuf {
        self.root.join(format!("{key_id}.json"))
    }

    fn latest_path(&self) -> PathBuf {
        self.root.join("latest.txt")
    }
}

impl SignedPrekeyStore for FileSignedPrekeyStore {
    fn save_signed_prekey(
        &mut self,
        key_id: &str,
        private_key: &PrivateKeyMaterial,
        metadata: &SignedPrekey,
    ) -> Result<(), DirectE2eeError> {
        Self::save_signed_prekey(self, key_id, private_key, metadata)
    }

    fn load_signed_prekey(&self, key_id: &str) -> Result<PrivateKeyMaterial, DirectE2eeError> {
        FileSignedPrekeyStore::load_signed_prekey(self, key_id).map(|(private_key, _)| private_key)
    }
}

#[allow(non_snake_case)]
pub fn NewFileSignedPrekeyStore(
    root: impl AsRef<Path>,
) -> Result<FileSignedPrekeyStore, DirectE2eeError> {
    FileSignedPrekeyStore::new(root)
}

#[derive(Debug, Clone)]
pub struct FileOneTimePrekeyStore {
    root: PathBuf,
}

impl FileOneTimePrekeyStore {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, DirectE2eeError> {
        let root = root.as_ref().to_path_buf();
        create_store_dir(&root)?;
        Ok(Self { root })
    }

    pub fn save_one_time_prekey(
        &mut self,
        key_id: &str,
        private_key: &PrivateKeyMaterial,
        metadata: &OneTimePrekey,
    ) -> Result<(), DirectE2eeError> {
        write_private_file(&self.pem_path(key_id), private_key.to_pem().as_bytes())?;
        write_public_json(&self.json_path(key_id), metadata)
    }

    pub fn load_one_time_prekey(
        &self,
        key_id: &str,
    ) -> Result<(PrivateKeyMaterial, OneTimePrekey), DirectE2eeError> {
        let pem_path = self.pem_path(key_id);
        let raw = match fs::read_to_string(&pem_path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(DirectE2eeError::invalid_field(format!(
                    "one-time prekey not found: {key_id}"
                )));
            }
            Err(err) => return Err(store_io_error(err)),
        };
        let private_key = PrivateKeyMaterial::from_pem(&raw)
            .map_err(|err| DirectE2eeError::invalid_field(err.to_string()))?;
        let metadata = read_json_file(&self.json_path(key_id))?;
        Ok((private_key, metadata))
    }

    pub fn list_one_time_prekeys(&self) -> Result<Vec<OneTimePrekey>, DirectE2eeError> {
        let mut result = Vec::new();
        for path in json_paths(&self.root)? {
            result.push(read_json_file(&path)?);
        }
        result.sort_by(|left: &OneTimePrekey, right| left.key_id.cmp(&right.key_id));
        Ok(result)
    }

    pub fn delete_one_time_prekey(&mut self, key_id: &str) -> Result<(), DirectE2eeError> {
        remove_file_if_exists(self.pem_path(key_id))?;
        remove_file_if_exists(self.json_path(key_id))
    }

    fn pem_path(&self, key_id: &str) -> PathBuf {
        self.root.join(format!("{key_id}.pem"))
    }

    fn json_path(&self, key_id: &str) -> PathBuf {
        self.root.join(format!("{key_id}.json"))
    }
}

#[allow(non_snake_case)]
pub fn NewFileOneTimePrekeyStore(
    root: impl AsRef<Path>,
) -> Result<FileOneTimePrekeyStore, DirectE2eeError> {
    FileOneTimePrekeyStore::new(root)
}

#[derive(Debug, Clone)]
pub struct FilePendingOutboundStore {
    root: PathBuf,
}

impl FilePendingOutboundStore {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, DirectE2eeError> {
        let root = root.as_ref().to_path_buf();
        create_store_dir(&root)?;
        Ok(Self { root })
    }

    pub fn save_pending(&mut self, pending: &PendingOutboundRecord) -> Result<(), DirectE2eeError> {
        write_public_json(&self.pending_path(&pending.operation_id), pending)
    }

    pub fn load_pending(
        &self,
        operation_id: &str,
    ) -> Result<PendingOutboundRecord, DirectE2eeError> {
        let path = self.pending_path(operation_id);
        let raw = match fs::read(&path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(DirectE2eeError::PendingOutboundNotFound(
                    operation_id.to_string(),
                ));
            }
            Err(err) => return Err(store_io_error(err)),
        };
        serde_json::from_slice(&raw).map_err(store_json_error)
    }

    pub fn delete_pending(&mut self, operation_id: &str) -> Result<(), DirectE2eeError> {
        remove_file_if_exists(self.pending_path(operation_id))
    }

    fn pending_path(&self, operation_id: &str) -> PathBuf {
        self.root.join(format!("{operation_id}.json"))
    }
}

impl PendingOutboundStore for FilePendingOutboundStore {
    fn save_pending(&mut self, pending: &PendingOutboundRecord) -> Result<(), DirectE2eeError> {
        Self::save_pending(self, pending)
    }

    fn load_pending(&self, operation_id: &str) -> Result<PendingOutboundRecord, DirectE2eeError> {
        Self::load_pending(self, operation_id)
    }

    fn delete_pending(&mut self, operation_id: &str) -> Result<(), DirectE2eeError> {
        Self::delete_pending(self, operation_id)
    }
}

#[allow(non_snake_case)]
pub fn NewFilePendingOutboundStore(
    root: impl AsRef<Path>,
) -> Result<FilePendingOutboundStore, DirectE2eeError> {
    FilePendingOutboundStore::new(root)
}

fn session_json_paths(root: &Path) -> Result<Vec<PathBuf>, DirectE2eeError> {
    json_paths(root)
}

fn json_paths(root: &Path) -> Result<Vec<PathBuf>, DirectE2eeError> {
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
    write_private_json(path, session)
}

fn write_private_json<T: Serialize>(path: &Path, value: &T) -> Result<(), DirectE2eeError> {
    let raw = serde_json::to_vec_pretty(value).map_err(store_json_error)?;
    write_private_file(path, &raw)?;
    Ok(())
}

fn write_public_json<T: Serialize>(path: &Path, value: &T) -> Result<(), DirectE2eeError> {
    let raw = serde_json::to_vec_pretty(value).map_err(store_json_error)?;
    write_public_file(path, &raw)
}

fn read_json_file<T: DeserializeOwned>(path: &Path) -> Result<T, DirectE2eeError> {
    let raw = fs::read(path).map_err(store_io_error)?;
    serde_json::from_slice(&raw).map_err(store_json_error)
}

fn remove_file_if_exists(path: PathBuf) -> Result<(), DirectE2eeError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(store_io_error(err)),
    }
}

fn store_io_error(err: std::io::Error) -> DirectE2eeError {
    DirectE2eeError::invalid_field(err.to_string())
}

fn store_json_error(err: serde_json::Error) -> DirectE2eeError {
    DirectE2eeError::invalid_field(err.to_string())
}

#[cfg(unix)]
fn create_session_dir(path: &Path) -> Result<(), DirectE2eeError> {
    create_store_dir(path)
}

#[cfg(unix)]
fn create_store_dir(path: &Path) -> Result<(), DirectE2eeError> {
    use std::os::unix::fs::DirBuilderExt;

    let mut builder = fs::DirBuilder::new();
    builder.recursive(true).mode(0o755);
    builder.create(path).map_err(store_io_error)
}

#[cfg(not(unix))]
fn create_session_dir(path: &Path) -> Result<(), DirectE2eeError> {
    create_store_dir(path)
}

#[cfg(not(unix))]
fn create_store_dir(path: &Path) -> Result<(), DirectE2eeError> {
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

#[cfg(unix)]
fn write_public_file(path: &Path, raw: &[u8]) -> Result<(), DirectE2eeError> {
    use std::os::unix::fs::OpenOptionsExt;

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o644)
        .open(path)
        .map_err(store_io_error)?;
    file.write_all(raw).map_err(store_io_error)
}

#[cfg(not(unix))]
fn write_public_file(path: &Path, raw: &[u8]) -> Result<(), DirectE2eeError> {
    fs::write(path, raw).map_err(store_io_error)
}
