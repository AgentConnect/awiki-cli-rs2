pub const MODULE_PATH: &str = "github.com/agent-network-protocol/anp/golang";
pub const MODULE_VERSION: &str = "v0.8.7";

pub use anp::authentication::{
    build_anp_message_service, create_did_wba_document, generate_auth_header,
    generate_http_signature_headers, resolve_did_document, resolve_did_document_with_options,
    AnpMessageServiceOptions, AuthMode, AuthenticationError, DIDWbaAuthHeader, DidDocumentBundle,
    DidDocumentOptions, DidProfile, DidResolutionOptions, DidWbaVerifier, DidWbaVerifierConfig,
    HttpSignatureOptions,
};
pub use anp::direct_e2ee::{
    DirectE2eeSession, DirectSessionState, OneTimePrekey, PrekeyBundle, SignedPrekey,
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

pub const AUTH_MODE_HTTP_SIGNATURES: AuthMode = AuthMode::HttpSignatures;
pub const AUTH_MODE_AUTO: AuthMode = AuthMode::Auto;
pub const DID_PROFILE_E1: DidProfile = DidProfile::E1;
pub const DID_PROFILE_K1: DidProfile = DidProfile::K1;
pub const TARGET_KIND_AGENT: TargetKind = TargetKind::Agent;
pub const TARGET_KIND_GROUP: TargetKind = TargetKind::Group;
pub const TARGET_KIND_SERVICE: TargetKind = TargetKind::Service;
