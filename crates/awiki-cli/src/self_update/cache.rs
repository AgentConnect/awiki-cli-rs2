use super::Metadata;
use crate::cli_http::{new_http_client_with_proxy_env, HttpRequest};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

#[derive(Debug, Deserialize)]
struct CacheFile {
    #[serde(default)]
    product: String,
    #[serde(default)]
    channel: String,
    #[serde(default)]
    policy_origin: String,
    #[serde(default)]
    policy_revision: u64,
    #[serde(default)]
    published_at: String,
    #[serde(default)]
    release_notes_url: String,
    #[serde(default)]
    latest_version: String,
    #[serde(default)]
    min_supported_version: String,
    #[serde(default)]
    installer_url: String,
    #[serde(default)]
    installer_mirrors: Vec<String>,
    #[serde(default)]
    installer_sha256: String,
    #[serde(default)]
    installer_size: u64,
    #[serde(default)]
    installer_integrity: String,
    #[serde(default)]
    retrieved_at: String,
}

#[derive(Debug, Clone)]
struct CacheRead {
    metadata: Metadata,
    fresh: bool,
}

pub fn load_metadata(
    cache_dir: Option<&Path>,
    ttl_seconds: i64,
    prefer_fresh: bool,
    cache_only: bool,
    registry_urls: &[String],
    expected_policy_origin: &str,
) -> Result<Metadata, String> {
    let cache_path = cache_path(cache_dir);
    let mut cached = cache_path
        .as_deref()
        .and_then(|path| {
            read_cache(path, ttl_seconds, expected_policy_origin)
                .ok()
                .flatten()
        })
        .filter(|cache| !cache.metadata.latest_version.trim().is_empty());

    if let Some(cache) = cached.as_mut() {
        if cache.fresh {
            cache.metadata.source = "cache".to_string();
            if !prefer_fresh || cache_only {
                return Ok(cache.metadata.clone());
            }
        } else if cache_only {
            cache.metadata.source = "cache_stale".to_string();
            return Ok(cache.metadata.clone());
        }
    }

    if cache_only {
        return Err(
            "update cache-only mode is enabled but no cached metadata is available".to_string(),
        );
    }

    match fetch_from_registry(registry_urls, expected_policy_origin) {
        Ok(network) => {
            if cached.as_ref().is_some_and(|cached| {
                cached.metadata.policy_origin == network.policy_origin
                    && cached.metadata.policy_revision > network.policy_revision
            }) {
                let mut metadata = cached.expect("cached metadata checked above").metadata;
                metadata.source = "cache_stale".to_string();
                return Ok(metadata);
            }
            if let Some(cache_path) = cache_path.as_deref() {
                let _ = write_cache(cache_path, &network);
            }
            Ok(network)
        }
        Err(err) => {
            if err.contains("status 404") {
                return Err(err);
            }
            if let Some(cache) = cached {
                let mut metadata = cache.metadata;
                metadata.source = "cache_stale".to_string();
                Ok(metadata)
            } else {
                Err(err)
            }
        }
    }
}

fn fetch_from_registry(
    manifest_urls: &[String],
    expected_policy_origin: &str,
) -> Result<Metadata, String> {
    fetch_from_registry_urls(manifest_urls, expected_policy_origin)
}

fn fetch_from_registry_urls(
    registry_urls: &[String],
    expected_policy_origin: &str,
) -> Result<Metadata, String> {
    if registry_urls.is_empty() {
        return Err("no awiki-cli manifest URLs configured".to_string());
    }

    let mut errors = Vec::new();
    for url in registry_urls {
        match fetch_from_registry_url(url, expected_policy_origin) {
            Ok(metadata) => return Ok(metadata),
            Err(err) => errors.push(format!("{url}: {err}")),
        }
    }

    Err(format!(
        "failed to fetch awiki-cli release manifest: {}",
        errors.join("; ")
    ))
}

fn fetch_from_registry_url(url: &str, expected_policy_origin: &str) -> Result<Metadata, String> {
    let client = new_http_client_with_proxy_env("").map_err(|err| err.to_string())?;
    let response = client
        .execute(HttpRequest::new("GET", url).header("Accept", "application/json"))
        .map_err(|err| err.to_string())?;
    if response.status_code != 200 {
        return Err(format!(
            "release server responded with status {}",
            response.status_code
        ));
    }
    if response.body.len() > 1024 * 1024 {
        return Err("release manifest response exceeds 1 MiB".to_string());
    }

    let value: serde_json::Value =
        serde_json::from_slice(&response.body).map_err(|err| err.to_string())?;
    if value.get("client_versions").is_some() {
        return metadata_from_server_info(value, expected_policy_origin);
    }
    let body: ManifestResponse = serde_json::from_value(value).map_err(|err| err.to_string())?;
    let latest = if body.latest.trim().is_empty() {
        body.version.trim().to_string()
    } else {
        body.latest.trim().to_string()
    };
    if latest.is_empty() {
        return Err("release manifest missing latest version".to_string());
    }
    let product = if body.product.trim().is_empty() {
        "awiki-cli".to_string()
    } else {
        body.product.trim().to_string()
    };
    let channel = if body.channel.trim().is_empty() {
        "stable".to_string()
    } else {
        body.channel.trim().to_string()
    };
    let policy_origin = if body.policy_origin.trim().is_empty() {
        expected_policy_origin.to_string()
    } else {
        body.policy_origin.trim().trim_end_matches('/').to_string()
    };
    if product != "awiki-cli" || channel != "stable" {
        return Err("release manifest has the wrong product or channel".to_string());
    }
    if policy_origin != expected_policy_origin.trim_end_matches('/') {
        return Err(
            "release manifest policy_origin does not match the selected tenant".to_string(),
        );
    }
    let min_supported_version = if body.min_supported_version.trim().is_empty() {
        body.awiki_cli.min_supported_version.trim().to_string()
    } else {
        body.min_supported_version.trim().to_string()
    };
    let installer_url = if body.installer.url.trim().is_empty() {
        format!(
            "{}/awiki-cli.tgz",
            url.rsplit_once('/').map(|(base, _)| base).unwrap_or(url)
        )
    } else {
        body.installer.url.trim().to_string()
    };
    let installer_sha256 = body.installer.sha256.trim().to_ascii_lowercase();
    if !installer_sha256.is_empty()
        && (installer_sha256.len() != 64
            || !installer_sha256.chars().all(|ch| ch.is_ascii_hexdigit()))
    {
        return Err("release manifest installer SHA-256 is invalid".to_string());
    }
    let mut installer_mirrors = Vec::new();
    for mirror in body.installer.mirrors {
        if mirror.url.trim().is_empty() {
            return Err("release manifest contains an empty installer mirror".to_string());
        }
        if !mirror.sha256.trim().is_empty()
            && !installer_sha256.is_empty()
            && !mirror.sha256.eq_ignore_ascii_case(&installer_sha256)
        {
            return Err("installer mirrors must use the primary artifact SHA-256".to_string());
        }
        if mirror.size > 0 && body.installer.size > 0 && mirror.size != body.installer.size {
            return Err("installer mirrors must use the primary artifact size".to_string());
        }
        installer_mirrors.push(mirror.url.trim().to_string());
    }

    Ok(Metadata {
        product,
        channel,
        policy_origin,
        policy_revision: body.policy_revision.max(1),
        published_at: body.published_at.trim().to_string(),
        release_notes_url: body.release_notes_url.trim().to_string(),
        latest_version: latest,
        min_supported_version,
        installer_url,
        installer_mirrors,
        installer_sha256,
        installer_size: body.installer.size,
        installer_integrity: body.installer.integrity.trim().to_string(),
        source: "network".to_string(),
    })
}

fn metadata_from_server_info(
    value: serde_json::Value,
    expected_policy_origin: &str,
) -> Result<Metadata, String> {
    let body: ServerInfoResponse = serde_json::from_value(value).map_err(|err| err.to_string())?;
    if body.schema_version != 1 {
        return Err("server-info has an unsupported schema version".to_string());
    }
    let releases = body
        .client_versions
        .ok_or_else(|| "selected tenant does not publish a CLI update policy".to_string())?;
    if releases.schema_version != 1
        || releases.channel != "stable"
        || releases.policy_origin.trim_end_matches('/')
            != expected_policy_origin.trim_end_matches('/')
    {
        return Err("server-info CLI policy does not match the selected tenant".to_string());
    }
    let cli = releases.products.cli;
    if !cli.enabled {
        return Err("selected tenant does not publish a CLI update policy".to_string());
    }
    let latest = required(cli.recommended_version, "recommended_version")?;
    let minimum = required(cli.minimum_supported_version, "minimum_supported_version")?;
    if super::version::compare_versions(&minimum, &latest).is_none_or(|order| order > 0) {
        return Err("CLI minimum version exceeds its recommended version".to_string());
    }
    if cli.package_name.as_deref().unwrap_or_default() != "awiki-cli" {
        return Err("server-info has the wrong CLI package name".to_string());
    }
    let release_notes_url = required(cli.release_notes_url, "release_notes_url")?;
    let installer = cli
        .installer
        .ok_or_else(|| "server-info CLI policy is missing installer".to_string())?;
    validate_digest(&installer.sha256)?;
    if installer.size_bytes == 0 {
        return Err("server-info CLI installer size must be positive".to_string());
    }
    let installer_url = required(Some(installer.url), "installer.url")?;
    let mut mirrors = Vec::with_capacity(installer.mirrors.len());
    for mirror in installer.mirrors {
        mirrors.push(required(Some(mirror), "installer mirror")?);
    }
    Ok(Metadata {
        product: "awiki-cli".to_string(),
        channel: releases.channel,
        policy_origin: releases.policy_origin.trim_end_matches('/').to_string(),
        policy_revision: releases.policy_revision,
        published_at: releases.published_at,
        release_notes_url,
        latest_version: latest,
        min_supported_version: minimum,
        installer_url,
        installer_mirrors: mirrors,
        installer_sha256: installer.sha256.to_ascii_lowercase(),
        installer_size: installer.size_bytes,
        installer_integrity: installer.integrity.unwrap_or_default(),
        source: "network".to_string(),
    })
}

fn required(value: Option<String>, field: &str) -> Result<String, String> {
    let value = value.unwrap_or_default().trim().to_string();
    if value.is_empty() {
        Err(format!("server-info CLI policy is missing {field}"))
    } else {
        Ok(value)
    }
}

fn validate_digest(value: &str) -> Result<(), String> {
    if value.len() == 64 && value.chars().all(|ch| ch.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err("server-info CLI installer SHA-256 is invalid".to_string())
    }
}

#[derive(Debug, Deserialize)]
struct ServerInfoResponse {
    #[serde(default)]
    schema_version: u64,
    #[serde(default)]
    client_versions: Option<ServerClientVersions>,
}

#[derive(Debug, Deserialize)]
struct ServerClientVersions {
    #[serde(default)]
    schema_version: u64,
    #[serde(default)]
    channel: String,
    #[serde(default)]
    policy_origin: String,
    #[serde(default)]
    policy_revision: u64,
    #[serde(default)]
    published_at: String,
    products: ServerProducts,
}

#[derive(Debug, Deserialize)]
struct ServerProducts {
    cli: ServerCliRelease,
}

#[derive(Debug, Default, Deserialize)]
struct ServerCliRelease {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    package_name: Option<String>,
    #[serde(default)]
    recommended_version: Option<String>,
    #[serde(default)]
    minimum_supported_version: Option<String>,
    #[serde(default)]
    release_notes_url: Option<String>,
    #[serde(default)]
    installer: Option<ServerInstaller>,
}

#[derive(Debug, Deserialize)]
struct ServerInstaller {
    url: String,
    #[serde(default)]
    mirrors: Vec<String>,
    sha256: String,
    size_bytes: u64,
    #[serde(default)]
    integrity: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ManifestResponse {
    #[serde(default)]
    product: String,
    #[serde(default)]
    channel: String,
    #[serde(default)]
    policy_origin: String,
    #[serde(default)]
    policy_revision: u64,
    #[serde(default)]
    published_at: String,
    #[serde(default)]
    release_notes_url: String,
    #[serde(default)]
    latest: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    min_supported_version: String,
    #[serde(default)]
    installer: ManifestInstaller,
    #[serde(default, rename = "awikiCli")]
    awiki_cli: RegistryAwikiCli,
}

#[derive(Debug, Default, Deserialize)]
struct ManifestInstaller {
    #[serde(default)]
    url: String,
    #[serde(default)]
    sha256: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    integrity: String,
    #[serde(default)]
    mirrors: Vec<ManifestMirror>,
}

#[derive(Debug, Default, Deserialize)]
struct ManifestMirror {
    #[serde(default)]
    url: String,
    #[serde(default)]
    sha256: String,
    #[serde(default)]
    size: u64,
}

#[derive(Debug, Default, Deserialize)]
struct RegistryAwikiCli {
    #[serde(default, rename = "minSupportedVersion")]
    min_supported_version: String,
}

fn write_cache(path: &Path, metadata: &Metadata) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        set_dir_permissions(parent)?;
    }

    let payload = CacheWrite {
        product: metadata.product.as_str(),
        channel: metadata.channel.as_str(),
        policy_origin: metadata.policy_origin.as_str(),
        policy_revision: metadata.policy_revision,
        published_at: metadata.published_at.as_str(),
        release_notes_url: metadata.release_notes_url.as_str(),
        latest_version: metadata.latest_version.as_str(),
        min_supported_version: metadata.min_supported_version.as_str(),
        installer_url: metadata.installer_url.as_str(),
        installer_mirrors: &metadata.installer_mirrors,
        installer_sha256: metadata.installer_sha256.as_str(),
        installer_size: metadata.installer_size,
        installer_integrity: metadata.installer_integrity.as_str(),
        retrieved_at: OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|err| err.to_string())?,
        source: metadata.source.as_str(),
    };
    let raw = serde_json::to_string_pretty(&payload).map_err(|err| err.to_string())?;
    write_restricted_file(path, format!("{raw}\n").as_bytes())
}

#[derive(Debug, Serialize)]
struct CacheWrite<'a> {
    product: &'a str,
    channel: &'a str,
    policy_origin: &'a str,
    policy_revision: u64,
    published_at: &'a str,
    release_notes_url: &'a str,
    latest_version: &'a str,
    min_supported_version: &'a str,
    installer_url: &'a str,
    installer_mirrors: &'a [String],
    installer_sha256: &'a str,
    installer_size: u64,
    installer_integrity: &'a str,
    retrieved_at: String,
    source: &'a str,
}

fn write_restricted_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    write_restricted_file_direct(&temporary, bytes)?;
    fs::rename(&temporary, path).map_err(|err| {
        let _ = fs::remove_file(&temporary);
        err.to_string()
    })
}

fn write_restricted_file_direct(path: &Path, bytes: &[u8]) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(path)
            .map_err(|err| err.to_string())?;
        file.write_all(bytes).map_err(|err| err.to_string())?;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|err| err.to_string())?;
        return Ok(());
    }

    #[cfg(not(unix))]
    {
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(path)
            .map_err(|err| err.to_string())?;
        file.write_all(bytes).map_err(|err| err.to_string())
    }
}

fn set_dir_permissions(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|err| err.to_string())?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

fn cache_path(cache_dir: Option<&Path>) -> Option<PathBuf> {
    cache_dir
        .filter(|cache_dir| !cache_dir.as_os_str().is_empty())
        .map(|cache_dir| cache_dir.join("update").join("metadata.json"))
}

fn read_cache(
    path: &Path,
    ttl_seconds: i64,
    expected_policy_origin: &str,
) -> Result<Option<CacheRead>, String> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.to_string()),
    };
    let file: CacheFile = serde_json::from_str(&raw).map_err(|err| err.to_string())?;
    if file.product.trim() != "awiki-cli"
        || file.channel.trim() != "stable"
        || file.policy_revision < 1
        || file.policy_origin.trim().is_empty()
        || file.policy_origin.trim_end_matches('/') != expected_policy_origin.trim_end_matches('/')
        || super::version::compare_versions(
            file.min_supported_version.trim(),
            file.latest_version.trim(),
        )
        .is_none_or(|ordering| ordering > 0)
    {
        return Ok(None);
    }
    let retrieved_at = parse_retrieved_at(&file.retrieved_at)?;
    let fresh = match retrieved_at {
        Some(retrieved_at) if ttl_seconds > 0 => {
            OffsetDateTime::now_utc() - retrieved_at <= time::Duration::seconds(ttl_seconds)
        }
        _ => true,
    };
    Ok(Some(CacheRead {
        metadata: Metadata {
            product: file.product.trim().to_string(),
            channel: file.channel.trim().to_string(),
            policy_origin: file.policy_origin.trim().to_string(),
            policy_revision: file.policy_revision,
            published_at: file.published_at.trim().to_string(),
            release_notes_url: file.release_notes_url.trim().to_string(),
            latest_version: file.latest_version.trim().to_string(),
            min_supported_version: file.min_supported_version.trim().to_string(),
            installer_url: file.installer_url.trim().to_string(),
            installer_mirrors: file.installer_mirrors,
            installer_sha256: file.installer_sha256.trim().to_string(),
            installer_size: file.installer_size,
            installer_integrity: file.installer_integrity.trim().to_string(),
            source: "cache".to_string(),
        },
        fresh,
    }))
}

fn parse_retrieved_at(raw: &str) -> Result<Option<OffsetDateTime>, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    OffsetDateTime::parse(trimmed, &Rfc3339)
        .map(Some)
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_cache_directory_never_falls_back_to_the_working_directory() {
        assert_eq!(cache_path(None), None);
        assert_eq!(cache_path(Some(Path::new(""))), None);
        let cache_dir = std::env::temp_dir().join("awiki-cli-cache");
        assert_eq!(
            cache_path(Some(&cache_dir)),
            Some(cache_dir.join("update").join("metadata.json"))
        );
    }

    #[test]
    fn fetch_from_registry_rejects_missing_version() {
        let server = crate::self_update::tests::TestServer::new(vec![
            crate::self_update::tests::TestResponse::ok(
                r#"{"awikiCli":{"minSupportedVersion":"1.0.8"}}"#,
            ),
        ]);

        let err =
            fetch_from_registry_urls(&[server.url("/latest")], &server.url("")).expect_err("error");

        assert!(
            err.contains("release manifest missing latest version"),
            "error should report missing version: {err}"
        );
    }

    #[test]
    fn fetch_from_registry_allows_missing_min_supported_version() {
        let server = crate::self_update::tests::TestServer::new(vec![
            crate::self_update::tests::TestResponse::ok(r#"{"version":"1.0.9"}"#),
        ]);

        let metadata =
            fetch_from_registry_urls(&[server.url("/latest")], &server.url("")).expect("metadata");

        assert_eq!(metadata.latest_version, "1.0.9");
        assert_eq!(metadata.min_supported_version, "");
        assert_eq!(metadata.source, "network");
    }

    #[test]
    fn fetch_manifest_preserves_self_hosted_installer_url() {
        let server = crate::self_update::tests::TestServer::new(vec![
            crate::self_update::tests::TestResponse::ok(
                r#"{"schema_version":1,"latest":"1.0.17-beta.1","min_supported_version":"1.0.17-beta.1","installer":{"url":"https://example.com/cli/beta/awiki-cli.tgz"}}"#,
            ),
        ]);

        let metadata = fetch_from_registry_urls(&[server.url("/manifest.json")], &server.url(""))
            .expect("self-hosted manifest");

        assert_eq!(metadata.latest_version, "1.0.17-beta.1");
        assert_eq!(metadata.min_supported_version, "1.0.17-beta.1");
        assert_eq!(
            metadata.installer_url,
            "https://example.com/cli/beta/awiki-cli.tgz"
        );
    }
}
