use anp::group_e2ee::storage::ImCoreSqliteGroupMlsStore;

use super::native_provider::NativeAnpMlsProvider;
use super::DEFAULT_GROUP_MLS_DEVICE_ID;

pub(crate) fn native_provider_for_client(
    client: &crate::core::ImClient,
) -> crate::ImResult<NativeAnpMlsProvider> {
    let identity = client.current_identity();
    let device_id = identity
        .device_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(DEFAULT_GROUP_MLS_DEVICE_ID);
    let store = ImCoreSqliteGroupMlsStore::from_local_state_sqlite_path(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
        identity.id.as_str(),
        identity.did.as_str(),
        device_id,
    )
    .map_err(|err| crate::ImError::LocalStateUnavailable {
        detail: format!("initialize group MLS store: {err}"),
    })?;
    Ok(NativeAnpMlsProvider::new(store))
}

#[cfg(test)]
mod tests {
    use anp::group_e2ee::operations::{CreateGroupInput, FinalizeCommitInput, StatusInput};

    use super::*;
    use crate::internal::group_e2ee::provider::GroupMlsProvider;

    #[test]
    fn native_provider_for_client_uses_identity_scoped_im_core_store() {
        let root = unique_temp_root("im-core-group-e2ee-native-provider");
        let core = crate::core::ImCore::new(test_config(), test_paths(&root)).unwrap();
        let client = core
            .client(crate::identity::IdentitySelector::LocalAlias(
                "alice".to_owned(),
            ))
            .unwrap();
        let provider = native_provider_for_client(&client).expect("native provider");
        let group_did = "did:wba:example.com:groups:im-core-native-provider:e1";

        let create = provider
            .create_group_prepare(CreateGroupInput {
                creator_did: client.did().as_str().to_owned(),
                device_id: DEFAULT_GROUP_MLS_DEVICE_ID.to_owned(),
                group_did: group_did.to_owned(),
                operation_id: "op-im-core-native-create".to_owned(),
                request_id: "req-im-core-native-create".to_owned(),
                pending_commit_id: Some("pc-im-core-native-create".to_owned()),
            })
            .expect("create prepare");
        assert_eq!(create.status, "pending");
        assert_eq!(create.local_epoch, "0");

        let finalized = provider
            .finalize_commit(FinalizeCommitInput {
                pending_commit_id: create.pending_commit_id,
                request_id: "req-im-core-native-finalize".to_owned(),
            })
            .expect("finalize create");
        assert_eq!(finalized.status, "finalized");

        let status = provider
            .status(StatusInput {
                request_id: "req-im-core-native-status".to_owned(),
                device_id: DEFAULT_GROUP_MLS_DEVICE_ID.to_owned(),
                agent_did: Some(client.did().as_str().to_owned()),
                group_did: Some(group_did.to_owned()),
            })
            .expect("status");
        assert_eq!(status.status, "active");
        assert_eq!(status.local_epoch.as_deref(), Some("0"));

        let group_mls_root = root.join("local").join("group_mls");
        assert!(
            group_mls_root.exists(),
            "native provider should derive internal scoped MLS storage below local state root"
        );
        let scoped_db_count = count_mls_state_files(&group_mls_root);
        assert_eq!(scoped_db_count, 1);
    }

    fn test_config() -> crate::ImCoreConfig {
        crate::ImCoreConfig {
            service_base_url: crate::ServiceEndpoint::parse("https://example.test").unwrap(),
            did_domain: "awiki.info".to_owned(),
            user_service_endpoint: None,
            mail_service_endpoint: None,
            message_service_endpoint: None,
            anp_service_endpoint: None,
            anp_service_did: None,
            ca_bundle: None,
            transport_policy: crate::MessageTransportPolicy::Auto,
        }
    }

    fn test_paths(root: &std::path::Path) -> crate::ImCorePaths {
        crate::ImCorePaths {
            identities: crate::IdentityRegistryPaths {
                identity_root_dir: root.join("identities"),
                registry_path: root.join("identities").join("registry.json"),
                default_identity_path: Some(root.join("identities").join("default")),
            },
            local_state: crate::LocalStatePaths {
                sqlite_path: root.join("local").join("im.sqlite"),
            },
            runtime: crate::RuntimePaths {
                cache_dir: root.join("cache"),
                temp_dir: root.join("tmp"),
            },
        }
    }

    fn unique_temp_root(prefix: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()))
    }

    fn count_mls_state_files(root: &std::path::Path) -> usize {
        std::fs::read_dir(root)
            .expect("read group_mls root")
            .filter_map(Result::ok)
            .filter(|entry| entry.path().join("mls_state.sqlite").exists())
            .count()
    }
}
