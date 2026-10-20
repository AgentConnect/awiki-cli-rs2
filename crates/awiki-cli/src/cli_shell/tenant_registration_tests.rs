use super::*;

#[test]
fn tenant_registration_errors_preserve_stable_codes_without_remote_messages() {
    for (code, status, retryable) in [
        ("tenant_registration.registration_not_allowed", 403, false),
        ("tenant_registration.revision_conflict", 409, false),
        ("tenant_registration.unavailable", 503, true),
    ] {
        let error = map_error(im_core::ImError::Service {
            code: Some(code.into()),
            status_code: Some(status),
            message: "private@example.test".into(),
            data: None,
        });
        assert_eq!(error.detail.code, code);
        assert_eq!(error.detail.retryable, retryable);
        assert!(!error.detail.message.contains("private@example.test"));
    }
}

fn command(name: &str, extra: &[(&str, &str)]) -> ParsedCommand {
    let mut command = ParsedCommand {
        name: name.into(),
        ..Default::default()
    };
    command
        .flags
        .insert("domain".into(), "tenant.example.org".into());
    for (name, value) in extra {
        command.flags.insert((*name).into(), (*value).into());
    }
    command
}

fn temporary() -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("awiki-registration-{}", uuid::Uuid::new_v4()));
    private_directory(&path).unwrap();
    path
}

#[test]
fn tenant_registration_configure_requires_explicit_revision_and_closed_modes() {
    assert!(parse_action(&command("site.registration.configure", &[("mode", "open")])).is_err());
    assert!(parse_action(&command(
        "site.registration.configure",
        &[("expected-revision", "1"), ("mode", "other")]
    ))
    .is_err());
    let action = parse_action(&command(
        "site.registration.configure",
        &[
            ("expected-revision", "2"),
            ("mode", "allowlist"),
            ("methods", "phone,email"),
        ],
    ))
    .unwrap();
    assert_eq!(dry_plan(&action)["plan"]["expected_revision"], 2);
}

#[test]
fn tenant_registration_private_exports_never_overwrite_and_full_values_stay_off_stdout() {
    let dir = temporary();
    let output = dir.join("contacts.json");
    let contacts = vec![AllowlistContact {
        contact_type: ContactType::Email,
        value: "allowed@example.org".into(),
    }];
    let summary = finish_export(
        &SiteDomain::parse("tenant.example.org").unwrap(),
        output.to_str().unwrap(),
        Some(2),
        &contacts,
    )
    .unwrap();
    assert!(!summary.to_string().contains("allowed@example.org"));
    assert!(String::from_utf8(private_read(&output, 4096).unwrap())
        .unwrap()
        .contains("allowed@example.org"));
    assert!(finish_export(
        &SiteDomain::parse("tenant.example.org").unwrap(),
        output.to_str().unwrap(),
        Some(2),
        &contacts
    )
    .is_err());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn tenant_registration_export_refuses_mixed_revisions_without_producing_output() {
    let mut contacts = vec![];
    let mut revision = Some(1);
    let page = im_core::tenant_registration::AllowlistPage {
        domain: SiteDomain::parse("tenant.example.org").unwrap(),
        revision: 2,
        entries: vec![],
        next_cursor: None,
    };
    assert!(append_export(&mut contacts, &mut revision, &page).is_err());
    assert!(contacts.is_empty());
}

#[test]
fn tenant_registration_operation_records_are_bound_to_request_hash_and_explicit_id() {
    let directory = temporary();
    let path = directory.join("op.json");
    let id = uuid::Uuid::new_v4().to_string();
    private_create(
        &path,
        serde_json::to_vec(&json!({"version":1,"operation_id":id,"request_hash":"first"}))
            .unwrap()
            .as_slice(),
    )
    .unwrap();
    assert_eq!(existing_operation(&path, "first", Some(&id)).unwrap(), id);
    assert!(existing_operation(&path, "second", Some(&id)).is_err());
    assert!(existing_operation(&path, "first", Some(&uuid::Uuid::new_v4().to_string())).is_err());
    fs::remove_dir_all(directory).unwrap();
}

#[cfg(unix)]
#[test]
fn tenant_registration_input_rejects_symlinks_and_broad_permissions() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let directory = temporary();
    let source = directory.join("source.json");
    let link = directory.join("link.json");
    private_create(&source, b"[]").unwrap();
    symlink(&source, &link).unwrap();
    assert!(private_read(&link, 4096).is_err());
    fs::set_permissions(&source, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(private_read(&source, 4096).is_err());
    fs::remove_dir_all(directory).unwrap();
}
