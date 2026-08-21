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
