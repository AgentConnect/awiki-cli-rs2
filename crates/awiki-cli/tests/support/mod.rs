#![allow(dead_code)]

use anp::authentication::{create_did_wba_document, DidDocumentOptions};
use awiki_cli::workspace_config::Paths;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct TestIdentity {
    pub identity_name: String,
    pub unique_id: String,
    pub did: String,
    pub identity_dir: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct TestIdentityOptions<'a> {
    pub identity_name: &'a str,
    pub handle: &'a str,
    pub display_name: &'a str,
    pub jwt_token: &'a str,
    pub make_default: bool,
}

pub fn write_ready_identity(workspace: &Path, options: TestIdentityOptions<'_>) -> TestIdentity {
    let workspace = tenant_workspace(workspace);
    let identity_name = required(options.identity_name, "identity_name");
    let handle = required(options.handle, "handle");
    let display_name = if options.display_name.trim().is_empty() {
        identity_name
    } else {
        options.display_name.trim()
    };
    let jwt_token = options.jwt_token.trim();
    let unique_id = format!(
        "e1_{}_{}",
        sanitize_component(handle),
        sanitize_component(identity_name)
    );
    let full_handle = format!("{handle}.awiki.ai");
    let bundle = create_did_wba_document(
        "awiki.ai",
        DidDocumentOptions {
            path_segments: vec!["user".to_string(), handle.to_string(), unique_id.clone()],
            domain: Some("awiki.ai".to_string()),
            challenge: Some(format!("{identity_name}-fixture")),
            ..DidDocumentOptions::default()
        },
    )
    .expect("generate test identity DID document");
    let did = bundle.did().expect("generated DID").to_string();
    let key1_private_pem = private_key(&bundle, "key-1");
    let key1_public_pem = public_key(&bundle, "key-1");
    let e2ee_signing_private_pem = private_key(&bundle, "key-2");
    let e2ee_agreement_private_pem = private_key(&bundle, "key-3");

    let identity_root = workspace.join("identities");
    let dir_name = identity_name.to_string();
    let identity_dir = identity_root.join(&dir_name);
    std::fs::create_dir_all(&identity_dir).expect("create identity dir");
    write_json(
        &identity_dir.join("identity.json"),
        &json!({
            "did": did,
            "unique_id": unique_id,
            "created_at": "2026-05-25T00:00:00Z",
            "user_id": format!("user-{handle}"),
            "name": display_name,
            "handle": handle,
            "full_handle": full_handle,
        }),
    );
    write_json(
        &identity_dir.join("auth.json"),
        &json!({ "jwt_token": if jwt_token.is_empty() { Value::Null } else { Value::String(jwt_token.to_string()) } }),
    );
    write_json(
        &identity_dir.join("did_document.json"),
        &bundle.did_document,
    );
    std::fs::write(identity_dir.join("key-1-private.pem"), &key1_private_pem)
        .expect("write key-1 private");
    std::fs::write(identity_dir.join("key-1-public.pem"), &key1_public_pem)
        .expect("write key-1 public");
    std::fs::write(
        identity_dir.join("e2ee-signing-private.pem"),
        &e2ee_signing_private_pem,
    )
    .expect("write e2ee signing private");
    std::fs::write(
        identity_dir.join("e2ee-agreement-private.pem"),
        &e2ee_agreement_private_pem,
    )
    .expect("write e2ee agreement private");

    upsert_registry_entry(
        &identity_root,
        RegistryEntry {
            identity_name,
            dir_name: &dir_name,
            did: &did,
            unique_id: &unique_id,
            display_name,
            handle,
            full_handle: &full_handle,
            make_default: options.make_default,
        },
    );

    TestIdentity {
        identity_name: identity_name.to_string(),
        unique_id,
        did,
        identity_dir,
    }
}

pub fn open_local_state(workspace: &Path) -> rusqlite::Connection {
    let database_file = test_paths(workspace).database_file;
    if let Some(parent) = Path::new(&database_file).parent() {
        std::fs::create_dir_all(parent).expect("create database parent");
    }
    let connection = rusqlite::Connection::open(database_file).expect("open local sqlite");
    im_core::compat::local_state::ensure_schema(&connection).expect("ensure local sqlite schema");
    connection
}

pub fn test_paths(workspace: &Path) -> Paths {
    let workspace = tenant_workspace(workspace);
    for directory in ["data", "runtime", "cache", "logs"] {
        std::fs::create_dir_all(workspace.join(directory)).expect("create workspace subdir");
    }
    Paths {
        workspace_home_dir: path_string(&workspace),
        root_dir: path_string(&workspace),
        config_dir: path_string(&workspace),
        data_dir: path_string(&workspace.join("data")),
        state_dir: path_string(&workspace.join("runtime")),
        cache_dir: path_string(&workspace.join("cache")),
        logs_dir: path_string(&workspace.join("logs")),
        config_file: path_string(&workspace.join("config.yaml")),
        identity_dir: path_string(&workspace.join("identities")),
        database_file: path_string(&workspace.join("data").join("awiki-cli.db")),
        legacy_credentials_dir: path_string(&workspace.join("legacy-credentials")),
        legacy_data_dir: path_string(&workspace.join("legacy-data")),
    }
}

pub fn tenant_workspace(product_home: &Path) -> PathBuf {
    let tenants_dir = product_home.join("tenants");
    let global = std::fs::read(product_home.join("global.json"))
        .ok()
        .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok());
    let registry = std::fs::read(tenants_dir.join("registry.json"))
        .ok()
        .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok());
    if let (Some(global), Some(registry)) = (global, registry) {
        let requested = global["active_tenant"]
            .as_str()
            .unwrap_or("builtin-primary");
        let active = registry["aliases"][requested].as_str().unwrap_or(requested);
        if let Some(profile) = registry["tenants"]
            .as_array()
            .and_then(|profiles| profiles.iter().find(|profile| profile["name"] == active))
        {
            return tenants_dir.join(profile["dir_name"].as_str().unwrap_or(active));
        }
    }
    if tenants_dir.join("builtin-primary").exists() {
        return tenants_dir.join("builtin-primary");
    }
    if tenants_dir.join("default").exists() && !tenants_dir.join("china").exists() {
        return tenants_dir.join("default");
    }
    if tenants_dir.join("china").exists() {
        return tenants_dir.join("china");
    }
    tenants_dir.join("builtin-primary")
}

pub fn tenant_config_path(product_home: &Path) -> PathBuf {
    tenant_workspace(product_home).join("config.yaml")
}

pub fn write_tenant_config(product_home: &Path, text: &str) {
    let path = tenant_config_path(product_home);
    write_config_text(&path, text);
}

pub fn write_default_tenant_registry(product_home: &Path, backend_base_url: &str, did_host: &str) {
    let now = "2026-05-25T00:00:00Z";
    write_json(
        &product_home.join("global.json"),
        &json!({
            "schema_version": 1,
            "active_tenant": "default",
        }),
    );
    write_json(
        &product_home.join("tenants").join("registry.json"),
        &json!({
            "schema_version": 1,
            "tenants": [{
                "name": "default",
                "display_name": "AWiki",
                "backend_base_url": backend_base_url.trim().trim_end_matches('/'),
                "did_host": normalize_test_host(did_host),
                "dir_name": "default",
                "created_at": now,
                "updated_at": now,
            }],
        }),
    );
}

pub fn set_secret_storage_mode(workspace: &Path, mode: &str) {
    assert!(
        matches!(mode, "file_compat" | "vault_preferred" | "vault_required"),
        "unsupported test secret storage mode: {mode}"
    );
    let config_path = tenant_config_path(workspace);
    let text = std::fs::read_to_string(&config_path).unwrap_or_default();
    if text.trim().is_empty() {
        write_config_text(&config_path, &format!("secret_storage:\n  mode: {mode}\n"));
        return;
    }

    let mut output = Vec::new();
    let mut in_secret_storage = false;
    let mut saw_secret_storage = false;
    let mut wrote_mode = false;

    for line in text.lines() {
        let is_top_level =
            !line.chars().next().is_some_and(char::is_whitespace) && !line.trim().is_empty();
        if in_secret_storage && is_top_level {
            if !wrote_mode {
                output.push(format!("  mode: {mode}"));
                wrote_mode = true;
            }
            in_secret_storage = false;
        }

        if line.trim() == "secret_storage:" && is_top_level {
            saw_secret_storage = true;
            in_secret_storage = true;
            wrote_mode = false;
            output.push(line.to_string());
            continue;
        }

        if in_secret_storage && line.trim_start().starts_with("mode:") {
            output.push(format!("  mode: {mode}"));
            wrote_mode = true;
            continue;
        }

        output.push(line.to_string());
    }

    if in_secret_storage && !wrote_mode {
        output.push(format!("  mode: {mode}"));
    }
    if !saw_secret_storage {
        output.push(String::new());
        output.push("secret_storage:".to_string());
        output.push(format!("  mode: {mode}"));
    }
    write_config_text(&config_path, &format!("{}\n", output.join("\n")));
}

pub fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

struct RegistryEntry<'a> {
    identity_name: &'a str,
    dir_name: &'a str,
    did: &'a str,
    unique_id: &'a str,
    display_name: &'a str,
    handle: &'a str,
    full_handle: &'a str,
    make_default: bool,
}

fn upsert_registry_entry(identity_root: &Path, entry: RegistryEntry<'_>) {
    std::fs::create_dir_all(identity_root).expect("create identity root");
    let registry_path = identity_root.join("index.json");
    let mut registry = std::fs::read(&registry_path)
        .ok()
        .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok())
        .filter(|value| {
            value
                .get("credentials")
                .and_then(Value::as_object)
                .is_some()
        })
        .unwrap_or_else(
            || json!({ "schema_version": 3, "default_credential_name": "", "credentials": {} }),
        );
    registry["schema_version"] = Value::from(3);
    registry["credentials"][entry.identity_name] = json!({
        "credential_name": entry.identity_name,
        "dir_name": entry.dir_name,
        "did": entry.did,
        "unique_id": entry.unique_id,
        "user_id": format!("user-{}", entry.handle),
        "name": entry.display_name,
        "handle": entry.handle,
        "full_handle": entry.full_handle,
        "created_at": "2026-05-25T00:00:00Z",
        "is_default": entry.make_default,
    });
    if entry.make_default
        || registry
            .get("default_credential_name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .is_empty()
    {
        registry["default_credential_name"] = Value::String(entry.identity_name.to_string());
        std::fs::write(
            identity_root.join("default"),
            format!("{}\n", entry.identity_name),
        )
        .expect("write default identity");
    }
    write_json(&registry_path, &registry);
}

fn write_json(path: &Path, value: &Value) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create json parent");
    }
    std::fs::write(
        path,
        serde_json::to_vec_pretty(value).expect("serialize json"),
    )
    .unwrap_or_else(|err| panic!("write {path:?}: {err}"));
}

fn write_config_text(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create config parent");
    }
    std::fs::write(path, text).unwrap_or_else(|err| panic!("write {path:?}: {err}"));
}

fn required<'a>(value: &'a str, field: &str) -> &'a str {
    let value = value.trim();
    assert!(!value.is_empty(), "{field} is required");
    value
}

fn sanitize_component(raw: &str) -> String {
    raw.trim()
        .to_ascii_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches(['.', '_', '-'])
        .to_string()
}

fn normalize_test_host(raw: &str) -> String {
    raw.trim().trim_end_matches('.').to_ascii_lowercase()
}

fn private_key(bundle: &anp::authentication::DidDocumentBundle, fragment: &str) -> String {
    bundle
        .private_key_pem(fragment)
        .unwrap_or_else(|| panic!("{fragment} private key"))
        .to_string()
}

fn public_key(bundle: &anp::authentication::DidDocumentBundle, fragment: &str) -> String {
    bundle
        .public_key_pem(fragment)
        .unwrap_or_else(|| panic!("{fragment} public key"))
        .to_string()
}
