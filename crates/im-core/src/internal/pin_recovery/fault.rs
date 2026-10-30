//! Explicit test-process cuts. Absent from ordinary product behavior.
use super::pending::Operation;
pub(super) fn cut(stage: &str, operation: &Operation) -> crate::ImResult<()> {
    #[cfg(feature = "internal-test-helpers")]
    {
        if std::env::var("AWIKI_PIN_RECOVERY_TEST_ENABLE").as_deref() == Ok("1")
            && std::env::var("AWIKI_PIN_RECOVERY_TEST_CUT").as_deref() == Ok(stage)
        {
            use std::io::Write;
            let path = std::env::var_os("AWIKI_PIN_RECOVERY_TEST_MARKER")
                .ok_or_else(|| super::failure("test_marker_required", false))?;
            let value = serde_json::json!({"schema_version":1,"cutpoint":stage,"operation_id":operation.id,"op_jkt":operation.remote.as_ref().map(|r|r.op_jkt.as_str()),"target_id":operation.target.target_id,"device_id":operation.custody_pending.as_ref().and_then(|p|p["device_proposal"]["device_id"].as_str())});
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(path)?;
            file.write_all(
                &serde_json::to_vec(&value)
                    .map_err(|_| super::failure("test_marker_unavailable", false))?,
            )?;
            file.sync_all()?;
            std::process::exit(86);
        }
    }
    #[cfg(not(feature = "internal-test-helpers"))]
    let _ = (stage, operation);
    Ok(())
}
pub(super) fn block_register2(realm_id: &str) -> bool {
    #[cfg(feature = "internal-test-helpers")]
    {
        return std::env::var("AWIKI_PIN_RECOVERY_TEST_ENABLE").as_deref() == Ok("1")
            && std::env::var("AWIKI_PIN_RECOVERY_TEST_BLOCK_REGISTER2_REALM_ID").as_deref()
                == Ok(realm_id);
    }
    #[cfg(not(feature = "internal-test-helpers"))]
    {
        let _ = realm_id;
        false
    }
}
