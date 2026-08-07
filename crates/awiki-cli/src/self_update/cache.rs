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
    latest_version: String,
    #[serde(default)]
    min_supported_version: String,
    #[serde(default)]
    installer_url: String,
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
) -> Result<Metadata, String> {
    let cache_path = cache_path(cache_dir);
    let mut cached = cache_path
        .as_deref()
        .and_then(|path| read_cache(path, ttl_seconds).ok().flatten())
        .filter(|cache| !cache.metadata.latest_version.trim().is_empty());

    if let Some(cache) = cached.as_mut() {
        if cache.fresh {
            cache.metadata.source = "cache".to_string();
            if !prefer_fresh || cache_only {
                return Ok(cache.metadata.clone());
            }
        }
    }

    if cache_only {
        return Err(
            "update cache-only mode is enabled but no cached metadata is available".to_string(),
        );
    }

    match fetch_from_registry(registry_urls) {
        Ok(network) => {
            if let Some(cache_path) = cache_path.as_deref() {
                let _ = write_cache(cache_path, &network);
            }
            Ok(network)
        }
        Err(err) => {
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

fn fetch_from_registry(manifest_urls: &[String]) -> Result<Metadata, String> {
    fetch_from_registry_urls(manifest_urls)
}

fn fetch_from_registry_urls(registry_urls: &[String]) -> Result<Metadata, String> {
    if registry_urls.is_empty() {
        return Err("no awiki-cli manifest URLs configured".to_string());
    }

    let mut errors = Vec::new();
    for url in registry_urls {
        match fetch_from_registry_url(url) {
            Ok(metadata) => return Ok(metadata),
            Err(err) => errors.push(format!("{url}: {err}")),
        }
    }

    Err(format!(
        "failed to fetch awiki-cli release manifest: {}",
        errors.join("; ")
    ))
}

fn fetch_from_registry_url(url: &str) -> Result<Metadata, String> {
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

    let body: ManifestResponse =
        serde_json::from_slice(&response.body).map_err(|err| err.to_string())?;
    let latest = if body.latest.trim().is_empty() {
        body.version.trim().to_string()
    } else {
        body.latest.trim().to_string()
    };
    if latest.is_empty() {
        return Err("release manifest missing latest version".to_string());
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

    Ok(Metadata {
        latest_version: latest,
        min_supported_version,
        installer_url,
        source: "network".to_string(),
    })
}

#[derive(Debug, Deserialize)]
struct ManifestResponse {
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
        latest_version: metadata.latest_version.as_str(),
        min_supported_version: metadata.min_supported_version.as_str(),
        installer_url: metadata.installer_url.as_str(),
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
    latest_version: &'a str,
    min_supported_version: &'a str,
    installer_url: &'a str,
    retrieved_at: String,
    source: &'a str,
}

fn write_restricted_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
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

fn read_cache(path: &Path, ttl_seconds: i64) -> Result<Option<CacheRead>, String> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.to_string()),
    };
    let file: CacheFile = serde_json::from_str(&raw).map_err(|err| err.to_string())?;
    let retrieved_at = parse_retrieved_at(&file.retrieved_at)?;
    let fresh = match retrieved_at {
        Some(retrieved_at) if ttl_seconds > 0 => {
            OffsetDateTime::now_utc() - retrieved_at <= time::Duration::seconds(ttl_seconds)
        }
        _ => true,
    };
    if !fresh {
        return Ok(None);
    }
    Ok(Some(CacheRead {
        metadata: Metadata {
            latest_version: file.latest_version.trim().to_string(),
            min_supported_version: file.min_supported_version.trim().to_string(),
            installer_url: file.installer_url.trim().to_string(),
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

        let err = fetch_from_registry_urls(&[server.url("/latest")]).expect_err("error");

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

        let metadata = fetch_from_registry_urls(&[server.url("/latest")]).expect("metadata");

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

        let metadata = fetch_from_registry_urls(&[server.url("/manifest.json")])
            .expect("self-hosted manifest");

        assert_eq!(metadata.latest_version, "1.0.17-beta.1");
        assert_eq!(metadata.min_supported_version, "1.0.17-beta.1");
        assert_eq!(
            metadata.installer_url,
            "https://example.com/cli/beta/awiki-cli.tgz"
        );
    }
}
