use anyhow::{bail, Context};
use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Debug, Clone)]
pub struct BuiltinTenant {
    pub display_name: String,
    pub backend_origin: String,
    pub did_host: String,
}

#[derive(Debug, Clone, Deserialize)]
struct DisplayName {
    #[serde(rename = "zh-CN")]
    zh_cn: String,
    en: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RawTenant {
    display_name: DisplayName,
    backend_origin: String,
    did_host: String,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BuiltinTenantSlot {
    Primary,
    Secondary,
}

#[derive(Debug, Deserialize)]
struct RawTenants {
    primary: RawTenant,
    secondary: RawTenant,
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    schema_version: u64,
    default_slot: BuiltinTenantSlot,
    tenants: RawTenants,
}

#[derive(Debug)]
pub struct BuiltinTenantCatalog {
    pub default_slot: BuiltinTenantSlot,
    pub primary: BuiltinTenant,
    pub secondary: BuiltinTenant,
}

pub fn catalog() -> &'static BuiltinTenantCatalog {
    static CATALOG: OnceLock<BuiltinTenantCatalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        decode(include_str!(concat!(
            env!("OUT_DIR"),
            "/builtin-tenants.json"
        )))
        .expect("build-validated AWiki tenant config")
    })
}

fn decode(raw: &str) -> anyhow::Result<BuiltinTenantCatalog> {
    let value: serde_json::Value =
        serde_json::from_str(raw).context("parse built-in tenant config")?;
    let tenant_keys = value
        .get("tenants")
        .and_then(serde_json::Value::as_object)
        .context("built-in tenant config is missing tenants")?;
    if tenant_keys.len() != 2
        || !tenant_keys.contains_key("primary")
        || !tenant_keys.contains_key("secondary")
    {
        bail!("built-in tenant config must contain exactly primary and secondary")
    }
    let raw: RawConfig = serde_json::from_value(value).context("decode built-in tenant config")?;
    if raw.schema_version != 1 {
        bail!("unsupported built-in tenant config schema")
    }
    let primary = validate_tenant(raw.tenants.primary)?;
    let secondary = validate_tenant(raw.tenants.secondary)?;
    if primary.backend_origin == secondary.backend_origin || primary.did_host == secondary.did_host
    {
        bail!("built-in tenant endpoints must be distinct")
    }
    Ok(BuiltinTenantCatalog {
        default_slot: raw.default_slot,
        primary,
        secondary,
    })
}

fn validate_tenant(tenant: RawTenant) -> anyhow::Result<BuiltinTenant> {
    let display_name = tenant.display_name.en.trim().to_string();
    if tenant.display_name.zh_cn.trim().is_empty() {
        bail!("built-in tenant Chinese display name must not be empty")
    }
    let mut tenant = BuiltinTenant {
        display_name,
        backend_origin: tenant.backend_origin,
        did_host: tenant.did_host,
    };
    tenant.backend_origin = tenant
        .backend_origin
        .trim()
        .trim_end_matches('/')
        .to_string();
    tenant.did_host = tenant
        .did_host
        .trim()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if tenant.display_name.is_empty() || tenant.did_host.is_empty() {
        bail!("built-in tenant fields must not be empty")
    }
    let expected = format!("https://{}", tenant.did_host);
    let loopback = matches!(tenant.did_host.as_str(), "localhost" | "127.0.0.1" | "::1")
        && tenant.backend_origin.starts_with("http://");
    if tenant.backend_origin != expected && !loopback {
        bail!("built-in tenant origin must be HTTPS and match did_host")
    }
    let authority = tenant
        .backend_origin
        .split_once("://")
        .map(|(_, value)| value)
        .unwrap_or_default();
    if authority.is_empty() || authority.contains(['/', '?', '#']) {
        bail!("built-in tenant backend must be an origin")
    }
    Ok(tenant)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_complete_override_without_official_domains() {
        let parsed = decode(r#"{"schema_version":1,"default_slot":"secondary","tenants":{"primary":{"display_name":{"zh-CN":"甲","en":"Alpha"},"backend_origin":"https://alpha.example","did_host":"alpha.example"},"secondary":{"display_name":{"zh-CN":"乙","en":"Beta"},"backend_origin":"https://beta.example","did_host":"beta.example"}}}"#).unwrap();
        assert_eq!(parsed.default_slot, BuiltinTenantSlot::Secondary);
        assert_eq!(parsed.primary.backend_origin, "https://alpha.example");
    }
}
