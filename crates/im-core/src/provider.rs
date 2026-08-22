//! Restricted host-side identity provider contract.
//!
//! This module is available only with the `provider-traits` feature. It is an
//! integration SPI for trusted hosts such as the Node External Provider
//! adapter, not the ordinary application-facing identity API.

pub use crate::internal::identity_provider::{
    IdentityCustody, IdentityProviderError, IdentityProviderErrorCode, IdentitySession,
    ProviderExactHttpRequest, ProviderHttpHeader, ProviderHttpSigningOptions,
    ProviderIdentityDescriptor, ProviderIdentityRef, ProviderIdentityState,
    ProviderKeyAgreementRequest, ProviderKeyAlgorithm, ProviderKeyPurpose, ProviderKeySelector,
    ProviderOriginProofOptions, ProviderOriginProofRequest, ProviderPreparedHttpSignature,
    ProviderPublicIdentity, ProviderPublicKey, ProviderResult, ProviderSharedSecret,
    ProviderSignRequest, ProviderSignature, ProviderSignedOriginProof, ProviderSigningPurpose,
    ProviderStoreInfo, CAP_HTTP_SIGN, CAP_IDENTITY_SIGN, CAP_KEY_AGREEMENT, CAP_ORIGIN_PROOF,
    CAP_STORE_READ, IDENTITY_PROVIDER_PROTOCOL,
};
