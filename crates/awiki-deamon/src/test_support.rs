//! Native executable wrappers for the existing offline ACP fixture.
use std::path::{Path, PathBuf};
pub(crate) fn write_python_fixture(root: &Path, name: &str, source: &str) -> PathBuf {
    assert!(!name.contains(['/', '\\', '"', '%']));
    let script = root.join(name);
    std::fs::write(&script, source).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        script
    }
    #[cfg(windows)]
    {
        let python = std::env::var_os("AWIKI_TEST_PYTHON_BIN").expect(
            "Windows ACP unit fixtures require AWIKI_TEST_PYTHON_BIN pointing to native python.exe",
        );
        let python = PathBuf::from(python);
        assert!(python.is_absolute() && python.is_file());
        let python = python.to_string_lossy();
        assert!(!python.contains(['"', '\r', '\n']));
        let launcher = script.with_extension("cmd");
        std::fs::write(
            &launcher,
            format!(
                "@echo off\r\nsetlocal DisableDelayedExpansion\r\n\"{}\" \"%~dp0{}\" %*\r\n",
                python.replace('%', "%%"),
                name
            ),
        )
        .unwrap();
        launcher
    }
}
