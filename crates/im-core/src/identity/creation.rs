use serde::{Deserialize, Serialize};

use super::DidMethod;

/// Methods available for new ordinary Handle identities. Existing identities
/// keep their method and are not disabled when a creation capability is closed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityCreationCapabilities {
    pub did_methods: Vec<DidMethod>,
}

/// Public continuation hint. Contact values, grants, Vault references and
/// operation records remain inside Core; retry the same registration entrypoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingIdentityRegistration {
    pub did: String,
    pub full_handle: String,
    pub method: DidMethod,
    pub display_name: String,
    pub verification_kind: String,
    pub phase: String,
}

impl IdentityCreationCapabilities {
    pub(crate) fn from_server_info(value: &serde_json::Value) -> crate::ImResult<Self> {
        let methods = match value.pointer("/identity/did_methods") {
            None => vec![DidMethod::Wba],
            Some(value) => {
                let methods = value.as_array().ok_or(crate::ImError::PermissionDenied)?;
                let mut supported = Vec::new();
                let legacy = methods.iter().all(serde_json::Value::is_string);
                let mut seen = std::collections::BTreeSet::new();
                for method in methods {
                    let (id, create) = if legacy {
                        (
                            method.as_str().ok_or(crate::ImError::PermissionDenied)?,
                            true,
                        )
                    } else {
                        let id = method
                            .get("id")
                            .and_then(serde_json::Value::as_str)
                            .ok_or(crate::ImError::PermissionDenied)?;
                        let create = method
                            .get("create")
                            .and_then(serde_json::Value::as_bool)
                            .ok_or(crate::ImError::PermissionDenied)?;
                        if !seen.insert(id) {
                            return Err(crate::ImError::PermissionDenied);
                        }
                        (id, create)
                    };
                    if !create {
                        continue;
                    }
                    match id {
                        "wba" => supported.push(DidMethod::Wba),
                        "web" => supported.push(DidMethod::Web),
                        _ => {}
                    }
                }
                let mut unique = Vec::new();
                for method in supported {
                    if !unique.contains(&method) {
                        unique.push(method);
                    }
                }
                unique
            }
        };
        Ok(Self {
            did_methods: methods,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn creation_capabilities_default_to_wba_and_intersect_supported_methods() {
        assert_eq!(
            IdentityCreationCapabilities::from_server_info(&json!({}))
                .unwrap()
                .did_methods,
            [DidMethod::Wba]
        );
        assert_eq!(
            IdentityCreationCapabilities::from_server_info(
                &json!({"identity":{"did_methods":["wba","web","future"]}})
            )
            .unwrap()
            .did_methods,
            [DidMethod::Wba, DidMethod::Web]
        );
        assert!(IdentityCreationCapabilities::from_server_info(
            &json!({"identity":{"did_methods":[]}})
        )
        .unwrap()
        .did_methods
        .is_empty());
        assert!(IdentityCreationCapabilities::from_server_info(
            &json!({"identity":{"did_methods":null}})
        )
        .is_err());
    }

    #[test]
    fn creation_capabilities_require_explicit_create_and_reject_ambiguous_entries() {
        let capabilities = IdentityCreationCapabilities::from_server_info(&json!({
            "identity": {"did_methods": [{"id":"wba","create":true},{"id":"web","create":false}]}
        }))
        .unwrap();
        assert_eq!(capabilities.did_methods, [DidMethod::Wba]);
        for methods in [
            json!([{"id":"web"}]),
            json!([{"id":"web","create":"true"}]),
            json!([{"id":"web","create":true},{"id":"web","create":false}]),
            json!(["web",{"id":"web","create":false}]),
        ] {
            assert!(IdentityCreationCapabilities::from_server_info(
                &json!({"identity":{"did_methods":methods}})
            )
            .is_err());
        }
    }
}
