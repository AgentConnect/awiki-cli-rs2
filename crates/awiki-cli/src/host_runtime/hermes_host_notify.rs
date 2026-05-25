use crate::cli_http::{new_http_client_with_proxy_env, HttpClient, HttpRequest};
use crate::workspace_config::{self, Resolved};
use anyhow::Context;
use sha2::{Digest, Sha256};
use std::env;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::host_notify::HostNotificationEvent;

pub const HERMES_NOTIFY_SECRET_ENV: &str = "AWIKI_HOST_NOTIFY_HERMES_SECRET";
pub const LEGACY_WEBHOOK_NOTIFY_SECRET_ENV: &str = "AWIKI_HOST_NOTIFY_WEBHOOK_SECRET";
pub const NOTIFY_TIMESTAMP_HEADER: &str = "X-Notify-Timestamp";
pub const NOTIFY_SIGNATURE_HEADER: &str = "X-Notify-Signature";
pub const SIGNATURE_PREFIX: &str = "sha256=";
pub const NOTIFY_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
pub struct HermesHostNotifySink {
    client: HttpClient,
    notify_url: String,
    secret: String,
}

pub fn new_hermes_host_notify_sink(
    resolved: &Resolved,
    config: &super::HermesConfig,
) -> anyhow::Result<HermesHostNotifySink> {
    let notify_url = config.notify_url.trim();
    if notify_url.is_empty() {
        anyhow::bail!("hermes host notify requires runtime.host_notify.hermes.notify_url");
    }
    validate_hermes_notify_url(notify_url)?;
    let secret = resolve_hermes_notify_secret(Some(resolved));
    if secret.is_empty() {
        anyhow::bail!(
            "hermes host notify requires runtime.host_notify.hermes.secret or {HERMES_NOTIFY_SECRET_ENV} (legacy: {LEGACY_WEBHOOK_NOTIFY_SECRET_ENV})"
        );
    }
    let client = new_http_client_with_proxy_env(&resolved.ca_bundle)
        .map_err(|err| anyhow::anyhow!("build hermes host notify request: {err}"))?;
    Ok(HermesHostNotifySink {
        client,
        notify_url: notify_url.to_string(),
        secret,
    })
}

impl HermesHostNotifySink {
    pub fn notify(&self, event: &HostNotificationEvent) -> anyhow::Result<()> {
        let raw_body = serde_json::to_vec(event)
            .map_err(|err| anyhow::anyhow!("marshal host notify event: {err}"))?;
        let timestamp = unix_timestamp_seconds().to_string();
        let request = HttpRequest::new("POST", &self.notify_url)
            .header("Content-Type", "application/json")
            .header(NOTIFY_TIMESTAMP_HEADER, &timestamp)
            .header(
                NOTIFY_SIGNATURE_HEADER,
                build_hermes_notify_signature_header(&raw_body, &timestamp, &self.secret),
            )
            .body(raw_body)
            .timeout(NOTIFY_TIMEOUT);
        let response = self
            .client
            .execute(request)
            .map_err(|err| anyhow::anyhow!("send hermes host notify request: {err}"))?;
        if (200..300).contains(&response.status_code) {
            return Ok(());
        }
        let raw_response = limit_body(&response.body, 2048);
        let body = String::from_utf8_lossy(&raw_response).trim().to_string();
        if body.is_empty() {
            anyhow::bail!("hermes host notify failed status={}", response.status_code);
        }
        anyhow::bail!(
            "hermes host notify failed status={}: {}",
            response.status_code,
            body
        );
    }

    pub fn close(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

pub fn build_hermes_notify_signature(raw_body: &[u8], timestamp: &str, secret: &str) -> String {
    let mut signing_input = Vec::with_capacity(timestamp.len() + 1 + raw_body.len());
    signing_input.extend_from_slice(timestamp.as_bytes());
    signing_input.push(b'.');
    signing_input.extend_from_slice(raw_body);
    hmac_sha256_hex(secret.as_bytes(), &signing_input)
}

pub fn build_hermes_notify_signature_header(
    raw_body: &[u8],
    timestamp: &str,
    secret: &str,
) -> String {
    format!(
        "{SIGNATURE_PREFIX}{}",
        build_hermes_notify_signature(raw_body, timestamp, secret)
    )
}

pub fn validate_hermes_notify_url(raw_url: &str) -> anyhow::Result<()> {
    let (scheme, rest) = raw_url.split_once(':').ok_or_else(|| {
        anyhow::anyhow!("runtime.host_notify.hermes.notify_url must use http or https")
    })?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        anyhow::bail!("runtime.host_notify.hermes.notify_url must use http or https");
    }
    let authority = rest
        .strip_prefix("//")
        .map(|rest| rest.split(['/', '?', '#']).next().unwrap_or_default())
        .unwrap_or_default();
    let host =
        hostname(authority).with_context(|| "parse runtime.host_notify.hermes.notify_url")?;
    if host.trim().is_empty() {
        anyhow::bail!("runtime.host_notify.hermes.notify_url must include a host");
    }
    Ok(())
}

pub fn resolve_hermes_notify_secret(resolved: Option<&Resolved>) -> String {
    resolve_hermes_notify_secret_with_source(resolved, super::hermes_bridge::DEFAULT_NOTIFY_URL).0
}

pub fn resolve_hermes_notify_secret_with_source(
    resolved: Option<&Resolved>,
    notify_url: &str,
) -> (String, String) {
    if let Some(resolved) = resolved {
        let config_file = resolved.paths.config_file.trim();
        if !config_file.is_empty() {
            let (file_config, _, error) = workspace_config::read_file_config(config_file);
            if error.is_empty() {
                let secret = file_config.runtime.host_notify.hermes.secret.trim();
                if !secret.is_empty() {
                    return (secret.to_string(), "config_file".to_string());
                }
                let secret = file_config.runtime.host_notify.webhook.secret.trim();
                if !secret.is_empty() {
                    return (secret.to_string(), "config_file".to_string());
                }
            }
        }
    }
    if !notify_url.trim().is_empty() {
        if let Ok(secret) = env::var(HERMES_NOTIFY_SECRET_ENV) {
            let secret = secret.trim();
            if !secret.is_empty() {
                return (secret.to_string(), "environment".to_string());
            }
        }
        if let Ok(secret) = env::var(LEGACY_WEBHOOK_NOTIFY_SECRET_ENV) {
            let secret = secret.trim();
            if !secret.is_empty() {
                return (secret.to_string(), "environment".to_string());
            }
        }
    }
    (String::new(), "unset".to_string())
}

fn hostname(authority: &str) -> anyhow::Result<&str> {
    if authority.is_empty() {
        return Ok("");
    }
    let without_userinfo = authority.rsplit('@').next().unwrap_or(authority);
    if without_userinfo.starts_with('[') {
        let Some((host, suffix)) = without_userinfo[1..].split_once(']') else {
            anyhow::bail!("missing ']' in host");
        };
        validate_host_chars(host)?;
        validate_port_suffix(suffix)?;
        return Ok(host);
    }
    if without_userinfo.contains(['[', ']']) {
        anyhow::bail!("invalid character in host name");
    }
    let (host, suffix) = without_userinfo
        .split_once(':')
        .map(|(host, port)| (host, Some(port)))
        .unwrap_or((without_userinfo, None));
    validate_host_chars(host)?;
    if let Some(port) = suffix {
        validate_port(port)?;
    }
    Ok(host)
}

fn validate_host_chars(host: &str) -> anyhow::Result<()> {
    if host
        .chars()
        .any(|ch| ch.is_ascii_control() || ch.is_whitespace() || ch == '\\')
    {
        anyhow::bail!("invalid character in host name");
    }
    Ok(())
}

fn validate_port_suffix(suffix: &str) -> anyhow::Result<()> {
    if suffix.is_empty() {
        return Ok(());
    }
    let Some(port) = suffix.strip_prefix(':') else {
        anyhow::bail!("invalid character after host");
    };
    validate_port(port)
}

fn validate_port(port: &str) -> anyhow::Result<()> {
    if !port.is_empty() && !port.chars().all(|ch| ch.is_ascii_digit()) {
        anyhow::bail!("invalid port after host");
    }
    Ok(())
}

fn hmac_sha256_hex(key: &[u8], message: &[u8]) -> String {
    const BLOCK_SIZE: usize = 64;
    let mut key_block = [0u8; BLOCK_SIZE];
    if key.len() > BLOCK_SIZE {
        let digest = Sha256::digest(key);
        key_block[..digest.len()].copy_from_slice(&digest);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }

    let mut inner_key = [0x36u8; BLOCK_SIZE];
    let mut outer_key = [0x5cu8; BLOCK_SIZE];
    for index in 0..BLOCK_SIZE {
        inner_key[index] ^= key_block[index];
        outer_key[index] ^= key_block[index];
    }

    let mut inner = Sha256::new();
    inner.update(inner_key);
    inner.update(message);
    let inner_digest = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(outer_key);
    outer.update(inner_digest);
    hex_lower(&outer.finalize())
}

fn unix_timestamp_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn limit_body(body: &[u8], limit: usize) -> Vec<u8> {
    body.iter().copied().take(limit).collect()
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}
