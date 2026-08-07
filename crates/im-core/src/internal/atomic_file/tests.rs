use super::*;

#[test]
fn consecutive_replacements_keep_only_complete_images() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("state.json");

    for (index, expected) in [b"first".as_slice(), b"second", b"third"]
        .into_iter()
        .enumerate()
    {
        let temporary = root.path().join(format!("state-{index}.tmp"));
        std::fs::write(&temporary, expected).unwrap();
        std::fs::File::open(&temporary).unwrap().sync_all().unwrap();

        replace(&temporary, &target).unwrap();

        assert!(std::fs::read(&target).unwrap() == expected);
        assert!(!temporary.exists());
    }
}
