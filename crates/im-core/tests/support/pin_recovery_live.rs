//! Shared test-only protected-input/Vault initialization. No product state machine.
use awiki_im_core as sdk;
use sdk::pin_recovery::*;
use serde::Deserialize;
use std::{
    io::Write,
    path::{Path, PathBuf},
};
use zeroize::{Zeroize, Zeroizing};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Input {
    pub(crate) service_base_url: String,
    pub(crate) did_domain: String,
    pub(crate) storage_a: PathBuf,
    pub(crate) storage_b: PathBuf,
    pub(crate) tenant: PinRecoveryTenantConfig,
    pub(crate) phone: String,
    pub(crate) otp: String,
    pub(crate) pin: String,
    pub(crate) full_handle: String,
    pub(crate) invite_code: Option<String>,
    pub(crate) register_identity: bool,
    #[serde(default)]
    pub(crate) multi_device_audience: Option<String>,
    #[serde(default)]
    pub(crate) storage_a_context: Option<String>,
    #[serde(default)]
    pub(crate) storage_b_context: Option<String>,
}
impl Drop for Input {
    fn drop(&mut self) {
        self.phone.zeroize();
        self.otp.zeroize();
        self.pin.zeroize();
        if let Some(code) = self.invite_code.as_mut() {
            code.zeroize();
        }
    }
}
pub(crate) fn take<T>(result: sdk::ImResult<T>, step: &str) -> T {
    match result {
        Ok(v) => v,
        Err(e) => {
            let code = match &e {
                sdk::ImError::Service {
                    code: Some(code), ..
                } if code.len() < 80
                    && code.bytes().any(|b| b.is_ascii_lowercase())
                    && code.bytes().all(|b| {
                        b.is_ascii_lowercase()
                            || b.is_ascii_digit()
                            || matches!(b, b'_' | b'.' | b'-')
                    }) =>
                {
                    code.as_str()
                }
                _ => "core_error",
            };
            let kind = match &e {
                sdk::ImError::InvalidInput { .. } => "invalid_input",
                sdk::ImError::Service { .. } => "service",
                sdk::ImError::PermissionDenied => "permission_denied",
                sdk::ImError::TransportUnavailable { .. } => "transport_unavailable",
                sdk::ImError::LocalStateUnavailable { .. } => "local_state_unavailable",
                sdk::ImError::UnsupportedCapability { .. } => "unsupported_capability",
                sdk::ImError::IdentityNotReady { .. } => "identity_not_ready",
                sdk::ImError::Serialization { .. } => "serialization",
                sdk::ImError::Io { .. } => "io",
                _ => "core_error",
            };
            let status = if let sdk::ImError::Service {
                status_code: Some(status),
                ..
            } = &e
            {
                *status
            } else {
                0
            };
            panic!("Core driver step failed: {step}; code={code}; category={kind}; status={status}")
        }
    }
}
pub(crate) fn key(path: &Path) -> [u8; 32] {
    std::fs::create_dir_all(path).expect("driver storage directory");
    let file = path.join("vault-root.key");
    if let Ok(metadata) = std::fs::metadata(&file) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                metadata.permissions().mode() & 0o077,
                0,
                "driver vault key must be private"
            );
        }
    }
    let bytes = match std::fs::read(&file) {
        Ok(v) => Zeroizing::new(v),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut value = Zeroizing::new(vec![0u8; 32]);
            use rand::RngCore;
            rand::rngs::OsRng.fill_bytes(&mut value);
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut output = options.open(file).expect("driver protected vault key");
            output.write_all(&value).expect("driver vault key write");
            output.sync_all().expect("driver vault key sync");
            value
        }
        Err(_) => panic!("driver vault key unavailable"),
    };
    bytes
        .as_slice()
        .try_into()
        .expect("driver vault key length")
}
pub(crate) fn core(input: &Input, path: &Path, label: &str) -> sdk::ImCore {
    core_with_ca(input, path, label, None)
}
pub(crate) fn core_with_ca(
    input: &Input,
    path: &Path,
    label: &str,
    ca_bundle: Option<String>,
) -> sdk::ImCore {
    let label = if path == input.storage_a {
        input.storage_a_context.as_deref().unwrap_or(label)
    } else if path == input.storage_b {
        input.storage_b_context.as_deref().unwrap_or(label)
    } else {
        label
    };
    let paths = sdk::ImCorePaths {
        identities: sdk::IdentityRegistryPaths {
            identity_root_dir: path.join("identities"),
            registry_path: path.join("identities/index.json"),
            default_identity_path: Some(path.join("identities/default")),
        },
        local_state: sdk::LocalStatePaths {
            sqlite_path: path.join("local/im.sqlite"),
        },
        runtime: sdk::RuntimePaths {
            cache_dir: path.join("cache"),
            temp_dir: path.join("tmp"),
        },
    };
    let mut options = sdk::ImCoreOpenOptions::default()
        .with_pin_recovery_tenants(vec![input.tenant.clone()])
        .with_multi_device_direct_e2ee_enabled(true)
        .with_identity_secret_vault(
            sdk::IdentitySecretStoragePolicy::VaultRequired,
            sdk::ImCoreSecretVaultOptions::new(
                sdk::vault::DeviceVaultRootKey::from_bytes(key(path)),
                path.join("vault"),
                format!("pin-driver-{label}"),
                format!("pin-driver-{label}"),
            ),
        );
    if let Some(audience) = input.multi_device_audience.as_ref() {
        options = options
            .with_multi_device_handle_recovery_enabled(true)
            .with_multi_device_audience(audience.clone());
    }
    take(
        sdk::ImCore::new_with_options(
            sdk::ImCoreConfig {
                service_base_url: take(
                    sdk::ServiceEndpoint::parse(&input.service_base_url),
                    "service endpoint",
                ),
                did_domain: input.did_domain.clone(),
                // Matches the current awiki-me/pubspec.yaml and App default release.
                client_version_info: Some(take(
                    sdk::ClientVersionInfo::new("awiki-me", "0815", "0.1.41", Some(53)),
                    "current App version metadata",
                )),
                user_service_endpoint: None,
                message_service_endpoint: None,
                mail_service_endpoint: None,
                anp_service_endpoint: None,
                anp_service_did: None,
                ca_bundle,
                transport_policy: sdk::MessageTransportPolicy::HttpOnly,
            },
            paths,
            options,
        ),
        "Core open",
    )
}
pub(crate) async fn phone_session(
    core: &sdk::ImCore,
    input: &Input,
    resume_operation_id: Option<PinRecoveryOperationId>,
) -> PhoneSessionSummary {
    let facade = core.pin_recovery();
    let challenge = take(
        facade
            .request_phone_code(PhoneChallengeRequest {
                tenant_id: input.tenant.tenant_id.clone(),
                phone: Zeroizing::new(input.phone.clone()),
                resume_operation_id,
            })
            .await,
        "phone challenge",
    );
    let discovery = take(
        facade
            .verify_phone_code(PhoneVerifyRequest {
                challenge: challenge.challenge_handle,
                code: take(SecretOtp::new(input.otp.as_bytes().to_vec()), "OTP format"),
            })
            .await,
        "phone verification",
    );
    let targets = take(
        facade
            .list_targets(discovery.discovery_handle.clone())
            .await,
        "target discovery",
    );
    let selected = targets
        .iter()
        .find(|t| t.full_handle == input.full_handle)
        .expect("driver exact target is authorized");
    take(
        facade
            .select_target(discovery.discovery_handle, selected.target_id.clone())
            .await,
        "target selection",
    )
}
pub(crate) async fn pending(
    core: &sdk::ImCore,
    input: &Input,
    kind: PinRecoveryKind,
) -> Option<PinRecoveryProgress> {
    let mut operations = take(
        core.pin_recovery().list_pending().await,
        "pending operations",
    )
    .into_iter()
    .filter(|p| p.kind == kind && p.target.full_handle == input.full_handle)
    .collect::<Vec<_>>();
    assert!(
        operations.len() <= 1,
        "one pending Core operation per target"
    );
    operations.pop()
}
pub(crate) fn read_input() -> Input {
    let path = std::env::var_os("AWIKI_PIN_RECOVERY_DRIVER_CONFIG")
        .expect("protected config path is required");
    let path = PathBuf::from(path);
    let metadata = std::fs::metadata(&path).expect("driver config metadata");
    assert!(metadata.is_file(), "driver config must be a regular file");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            metadata.permissions().mode() & 0o077,
            0,
            "driver config must be private"
        );
    }
    let raw = Zeroizing::new(std::fs::read(&path).expect("protected config"));
    let input: Input =
        serde_json::from_slice(&raw).unwrap_or_else(|_| panic!("driver config schema invalid"));
    input
}
