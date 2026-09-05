use serde_json::Value;
use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=AWIKI_CLI_TENANT_CONFIG_PATH");
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest directory"));
    let default_path = manifest.join("../../config/builtin-tenants.default.json");
    let path = env::var_os("AWIKI_CLI_TENANT_CONFIG_PATH")
        .map(PathBuf::from)
        .unwrap_or(default_path);
    println!("cargo:rerun-if-changed={}", path.display());
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read AWiki tenant config {}: {error}", path.display()));
    let value: Value = serde_json::from_str(&raw)
        .unwrap_or_else(|error| panic!("parse AWiki tenant config {}: {error}", path.display()));
    validate_shape(&value);
    let out = PathBuf::from(env::var("OUT_DIR").expect("out directory"));
    fs::write(out.join("builtin-tenants.json"), raw).expect("write embedded tenant config");
}

fn validate_shape(value: &Value) {
    assert_eq!(
        value.get("schema_version").and_then(Value::as_u64),
        Some(1),
        "tenant config schema_version must be 1"
    );
    assert!(
        matches!(
            value.get("default_slot").and_then(Value::as_str),
            Some("primary" | "secondary")
        ),
        "tenant config default_slot must be primary or secondary"
    );
    let tenants = value
        .get("tenants")
        .and_then(Value::as_object)
        .expect("tenant config must contain tenants");
    assert_eq!(
        tenants.len(),
        2,
        "tenant config must contain exactly two tenants"
    );
    let mut endpoints = Vec::new();
    for slot in ["primary", "secondary"] {
        let tenant = tenants
            .get(slot)
            .and_then(Value::as_object)
            .unwrap_or_else(|| panic!("tenant config is missing {slot}"));
        for key in ["backend_origin", "did_host"] {
            assert!(
                !tenant
                    .get(key)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim()
                    .is_empty(),
                "tenant {slot} is missing {key}"
            );
        }
        let names = tenant
            .get("display_name")
            .and_then(Value::as_object)
            .unwrap_or_else(|| panic!("tenant {slot} display_name must be an object"));
        for locale in ["zh-CN", "en"] {
            assert!(
                !names
                    .get(locale)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim()
                    .is_empty(),
                "tenant {slot} is missing display_name.{locale}"
            );
        }
        let origin = tenant["backend_origin"]
            .as_str()
            .expect("validated origin")
            .trim();
        let did_host = tenant["did_host"]
            .as_str()
            .expect("validated DID host")
            .trim();
        let (scheme, authority) = origin
            .split_once("://")
            .unwrap_or_else(|| panic!("tenant {slot} origin must be absolute"));
        assert!(
            !authority.is_empty() && !authority.contains(['/', '?', '#', '@']),
            "tenant {slot} backend must be an origin"
        );
        let host = if let Some(bracketed) = authority.strip_prefix('[') {
            bracketed
                .split_once(']')
                .map(|(host, _)| host)
                .unwrap_or_default()
        } else {
            authority
                .split_once(':')
                .map(|(host, _)| host)
                .unwrap_or(authority)
        };
        let loopback = matches!(host, "localhost" | "127.0.0.1" | "::1");
        assert!(
            scheme == "https" || (scheme == "http" && loopback),
            "tenant {slot} origin must use HTTPS except for loopback development"
        );
        assert_eq!(host, did_host, "tenant {slot} origin must match did_host");
        if !loopback {
            assert_eq!(
                authority, did_host,
                "tenant {slot} production origin must not use a port"
            );
        }
        endpoints.push((origin, did_host));
    }
    assert_ne!(
        endpoints[0], endpoints[1],
        "tenant endpoints must be distinct"
    );
}
