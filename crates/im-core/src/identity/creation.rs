use serde::{Deserialize, Serialize};

use super::DidMethod;

/// Methods available for new ordinary Handle identities. Existing identities
/// keep their method and are not disabled when a creation capability is closed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityCreationCapabilities {
    pub did_methods: Vec<DidMethod>,
}

impl IdentityCreationCapabilities {
    pub(crate) fn from_server_info(value: &serde_json::Value) -> crate::ImResult<Self> {
        let methods = match value.pointer("/identity/did_methods") {
            None => vec![DidMethod::Wba],
            Some(value) => {
                let methods = value.as_array().ok_or(crate::ImError::PermissionDenied)?;
                let mut supported = Vec::new();
                for method in methods {
                    match method.as_str().ok_or(crate::ImError::PermissionDenied)? {
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
}
