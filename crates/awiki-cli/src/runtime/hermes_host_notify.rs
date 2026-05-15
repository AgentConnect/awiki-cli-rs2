use crate::config::{self, Resolved};
use anyhow::Context;
use sha2::{Digest, Sha256};
use std::env;

pub const HERMES_NOTIFY_SECRET_ENV: &str = "AWIKI_HOST_NOTIFY_HERMES_SECRET";
pub const LEGACY_WEBHOOK_NOTIFY_SECRET_ENV: &str = "AWIKI_HOST_NOTIFY_WEBHOOK_SECRET";
pub const NOTIFY_TIMESTAMP_HEADER: &str = "X-Notify-Timestamp";
pub const NOTIFY_SIGNATURE_HEADER: &str = "X-Notify-Signature";
pub const SIGNATURE_PREFIX: &str = "sha256=";

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
    if let Some(resolved) = resolved {
        let config_file = resolved.paths.config_file.trim();
        if !config_file.is_empty() {
            let (file_config, _, error) = config::read_file_config(config_file);
            if error.is_empty() {
                let secret = file_config.runtime.host_notify.hermes.secret.trim();
                if !secret.is_empty() {
                    return secret.to_string();
                }
                let secret = file_config.runtime.host_notify.webhook.secret.trim();
                if !secret.is_empty() {
                    return secret.to_string();
                }
            }
        }
    }
    if let Ok(secret) = env::var(HERMES_NOTIFY_SECRET_ENV) {
        let secret = secret.trim();
        if !secret.is_empty() {
            return secret.to_string();
        }
    }
    env::var(LEGACY_WEBHOOK_NOTIFY_SECRET_ENV)
        .unwrap_or_default()
        .trim()
        .to_string()
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

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}
