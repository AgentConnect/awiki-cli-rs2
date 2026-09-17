#[test]
fn migration_parser_accepts_explicit_credentials_source() {
    let parsed = super::parse_args(
        [
            "--migration",
            "id",
            "import-v1",
            "--name",
            "alice",
            "--credentials-dir",
            "/tmp/legacy",
        ]
        .into_iter()
        .map(str::to_owned),
    )
    .unwrap();
    assert_eq!(parsed.name, "id.import-v1");
    assert_eq!(parsed.flags["credentials-dir"], "/tmp/legacy");
}
