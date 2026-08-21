use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn im_core_src_does_not_reference_cli_boundary_types() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let src_dir = manifest_dir.join("src");
    let forbidden = [
        concat!("Parsed", "Command"),
        concat!("Exit", "Error"),
        concat!("Global", "Options"),
        concat!("config", "::", "Resolved"),
        concat!("identity", "::", "Manager"),
        concat!("awiki", "_", "cli"),
        concat!("crate", "::", "app"),
        concat!("crate", "::", "cli"),
        concat!("Actor", "Context"),
        concat!("Identity", "Runtime", "Paths"),
        concat!("SQLite", " connection"),
        concat!("raw ", "serde_json", " payload"),
    ];

    let mut failures = Vec::new();
    for path in rust_sources(&src_dir) {
        let source = fs::read_to_string(&path).unwrap();
        for needle in forbidden {
            if source.contains(needle) {
                failures.push(format!("{} contains {needle}", path.display()));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "im-core boundary violations:\n{}",
        failures.join("\n")
    );
}

#[test]
fn pending_identity_records_do_not_embed_private_key_fields() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let cases = [
        (
            "src/internal/identity_registration_pending.rs",
            &[
                "struct PendingRegistration",
                "struct PendingRegistrationIdentity",
            ][..],
        ),
        (
            "src/internal/identity_join_activation_pending.rs",
            &["struct PendingJoinActivation", "struct JoinEnrollmentRef"][..],
        ),
        (
            "src/internal/identity_handle_recovery_pending.rs",
            &[
                "struct PendingHandleRecoveryV4",
                "struct HandleRecoveryIdentityRef",
            ][..],
        ),
        (
            "src/internal/identity_legacy_upgrade_pending.rs",
            &[
                "struct PendingLegacyUpgrade",
                "struct LegacyUpgradeIdentityRef",
            ][..],
        ),
        (
            "src/internal/identity_root_import_completion.rs",
            &["struct RootImportSealedPlan", "struct RootImportCustodyRef"][..],
        ),
    ];
    let forbidden = [
        "private_key",
        "private_pem",
        "private_material",
        "generated:",
        "GeneratedVNextIdentityWithDaemonSubkey",
        "GeneratedIdentityWithDaemonSubkey",
        "DaemonSubkeyPrivatePackage",
    ];
    let mut failures = Vec::new();

    for (relative, declarations) in cases {
        let path = manifest_dir.join(relative);
        let source = fs::read_to_string(&path).unwrap();
        for declaration in declarations {
            let item = rust_item(&source, declaration)
                .unwrap_or_else(|| panic!("{} is missing {declaration}", path.display()));
            for needle in forbidden {
                if item.contains(needle) {
                    failures.push(format!(
                        "{} {declaration} contains identity private field marker {needle}",
                        path.display()
                    ));
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "pending identity custody boundary violations:\n{}",
        failures.join("\n")
    );
}

#[test]
fn root_transfer_sender_uses_only_confirmed_legacy_envelopes() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/internal/identity_root_transfer_runtime.rs");
    let source = fs::read_to_string(&path).unwrap();
    let production = source.split("#[cfg(test)]").next().unwrap_or(&source);
    for forbidden in ["export_wrapped_root", "RootTransferExportSpec"] {
        assert!(
            !production.contains(forbidden),
            "Root Transfer sender still contains wrapped send marker {forbidden}"
        );
    }
    for required in [
        "RootKeyEnvelopeV1",
        "root_private_key_pkcs8_b64u",
        "export_root_private_key",
        "user_presence_confirmed",
        "envelope_format = 'legacy_v1'",
    ] {
        assert!(
            production.contains(required),
            "Root Transfer sender is missing legacy send marker {required}"
        );
    }
    let confirmation = production
        .find("if !request.user_presence_confirmed")
        .expect("root transfer confirmation gate");
    let export = production
        .find(".export_root_private_key")
        .expect("ANP Identity root export");
    assert!(
        confirmation < export,
        "Root Transfer must verify user confirmation before exporting the root key"
    );
}

#[test]
fn dart_and_flutter_identity_surfaces_have_no_private_daemon_subkey_api() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let roots = [
        workspace.join("crates/im-core-dart/src"),
        workspace.join("packages/awiki_im_core/lib"),
    ];
    let forbidden = [
        "DaemonSubkeyPrivatePackage",
        "loadDaemonSubkeyPackage",
        "ensureDaemonSubkeyPackage",
        "load_daemon_subkey_package",
        "ensure_daemon_subkey_package",
        "privateKeyPem",
        "privateKeyMultibase",
        "privateKeyEncoding",
    ];
    let mut failures = Vec::new();
    for root in roots {
        for path in rust_and_dart_sources(&root) {
            let source = fs::read_to_string(&path).unwrap();
            for needle in forbidden {
                if source.contains(needle) {
                    failures.push(format!("{} contains {needle}", path.display()));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "public daemon custody boundary violations:\n{}",
        failures.join("\n")
    );
}

#[test]
fn rust_identity_public_surface_has_no_daemon_private_package_api() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for relative in ["src/identity/mod.rs", "src/prelude.rs", "src/lib.rs"] {
        let path = manifest_dir.join(relative);
        let source = fs::read_to_string(&path).unwrap();
        let public_source = source.split("pub(crate) use").next().unwrap_or(&source);
        assert!(
            !public_source.contains("DaemonSubkeyPrivatePackage"),
            "{} exports DaemonSubkeyPrivatePackage",
            path.display()
        );
    }
    let registry = fs::read_to_string(manifest_dir.join("src/identity/registry.rs")).unwrap();
    for forbidden in [
        "pub fn load_daemon_subkey_package",
        "pub async fn load_daemon_subkey_package_async",
        "pub fn ensure_daemon_subkey_package",
        "pub async fn ensure_daemon_subkey_package_async",
    ] {
        assert!(
            !registry.contains(forbidden),
            "registry contains {forbidden}"
        );
    }
}

#[test]
fn identity_custody_status_is_a_secret_free_projection() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/identity/dto.rs");
    let source = fs::read_to_string(&path).unwrap();
    let item = rust_item(&source, "struct IdentityCustodyStatus")
        .expect("IdentityCustodyStatus public DTO");
    for forbidden in [
        "private",
        "secret_ref",
        "jwt",
        "token",
        "fingerprint",
        "document_hash",
        "checkpoint",
        "provider_key",
    ] {
        assert!(
            !item.to_ascii_lowercase().contains(forbidden),
            "IdentityCustodyStatus contains forbidden field marker {forbidden}"
        );
    }
}

fn rust_item<'a>(source: &'a str, declaration: &str) -> Option<&'a str> {
    let start = source.find(declaration)?;
    let open = start + source[start..].find('{')?;
    let mut depth = 0_u32;
    for (offset, character) in source[open..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(&source[start..open + offset + character.len_utf8()]);
                }
            }
            _ => {}
        }
    }
    None
}

fn rust_sources(dir: &Path) -> Vec<PathBuf> {
    let mut sources = Vec::new();
    visit_rust_sources(dir, &mut sources);
    sources
}

fn rust_and_dart_sources(dir: &Path) -> Vec<PathBuf> {
    let mut sources = Vec::new();
    collect_sources(dir, &mut sources, &["rs", "dart"]);
    sources
}

fn collect_sources(dir: &Path, sources: &mut Vec<PathBuf>, extensions: &[&str]) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_sources(&path, sources, extensions);
        } else if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extensions.contains(&extension))
        {
            sources.push(path);
        }
    }
}

fn visit_rust_sources(dir: &Path, sources: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            visit_rust_sources(&path, sources);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            sources.push(path);
        }
    }
}
