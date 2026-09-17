use super::*;

#[test]
fn explicit_import_source_is_required_when_tenant_discovery_is_disabled() {
    let mut source = String::new();
    assert!(select_credentials_directory(&mut source, None).is_err());
    assert!(source.is_empty());
    let directory = std::env::current_dir().unwrap();
    select_credentials_directory(&mut source, directory.to_str()).unwrap();
    assert_eq!(Path::new(&source), directory.canonicalize().unwrap());
}

#[test]
fn invalid_override_never_falls_back_to_discovered_credentials() {
    let directory = std::env::current_dir().unwrap();
    let file = directory.join("Cargo.toml");
    assert!(file.is_file());
    for invalid in ["", " ", file.to_str().unwrap()] {
        let mut source = "existing-discovery".to_string();
        assert!(select_credentials_directory(&mut source, Some(invalid)).is_err());
        assert_eq!(source, "existing-discovery");
    }
}

#[test]
fn existing_allowed_discovery_is_preserved_without_override() {
    let mut source = "existing-discovery".to_string();
    select_credentials_directory(&mut source, None).unwrap();
    assert_eq!(source, "existing-discovery");
}
