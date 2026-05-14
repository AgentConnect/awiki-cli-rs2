use super::Metadata;
use serde::Deserialize;
use std::fs;
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
    retrieved_at: String,
}

#[derive(Debug, Clone)]
struct CacheRead {
    metadata: Metadata,
    fresh: bool,
}

pub fn load_metadata(
    cache_dir: &str,
    ttl_seconds: i64,
    prefer_fresh: bool,
    cache_only: bool,
) -> Result<Metadata, String> {
    let mut cached = read_cache(&cache_path(cache_dir), ttl_seconds)
        .ok()
        .flatten()
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

    if let Some(mut cache) = cached {
        cache.metadata.source = "cache_stale".to_string();
        return Ok(cache.metadata);
    }

    Err("network update checks are not implemented in this Rust port slice".to_string())
}

fn cache_path(cache_dir: &str) -> PathBuf {
    Path::new(cache_dir).join("update").join("metadata.json")
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
