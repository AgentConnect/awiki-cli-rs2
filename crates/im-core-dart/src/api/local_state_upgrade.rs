use crate::dto::{
    config::DartImCorePaths,
    error::DartImError,
    local_state_upgrade::{DartLocalStateUpgradeInspection, DartLocalStateUpgradeResult},
};

pub fn inspect_local_state_upgrade(
    paths: DartImCorePaths,
) -> Result<DartLocalStateUpgradeInspection, DartImError> {
    let paths: im_core::ImCorePaths = paths.try_into()?;
    im_core::inspect_local_state_upgrade(&paths.local_state)
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn upgrade_local_state(
    paths: DartImCorePaths,
) -> Result<DartLocalStateUpgradeResult, DartImError> {
    let paths: im_core::ImCorePaths = paths.try_into()?;
    tokio::task::spawn_blocking(move || im_core::upgrade_local_state(&paths.local_state))
        .await
        .map_err(|_| DartImError::internal("local state upgrade worker failed"))?
        .map(Into::into)
        .map_err(DartImError::from)
}
