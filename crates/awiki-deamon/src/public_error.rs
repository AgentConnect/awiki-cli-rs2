use anyhow::Chain;

const PUBLIC_ERROR_MAX_CHARS: usize = 240;
const PUBLIC_ERROR_CHAIN_MAX_CHARS: usize = 360;
const PUBLIC_URL_MAX_CHARS: usize = 512;
const PUBLIC_INVALID_URL_LABEL: &str = "<invalid-url>";

pub(crate) fn sanitize_public_error(message: &str) -> String {
    let mut redact_next = false;
    let mut parts = Vec::new();
    for part in message.split_whitespace() {
        if redact_next {
            push_public_error_part(&mut parts, "<redacted>");
            redact_next = secret_marker_requires_next_value(part);
            continue;
        }
        let sanitized = sanitize_public_error_part(part);
        if sanitized == "<redacted>" && secret_marker_requires_next_value(part) {
            redact_next = true;
        }
        push_public_error_part(&mut parts, sanitized);
    }
    let mut sanitized = parts.join(" ");
    if sanitized.chars().count() > PUBLIC_ERROR_MAX_CHARS {
        sanitized = sanitized.chars().take(PUBLIC_ERROR_MAX_CHARS).collect();
    }
    sanitized
}

pub(crate) fn sanitize_public_error_chain(chain: Chain<'_>) -> String {
    let parts = chain
        .take(4)
        .map(|error| sanitize_public_error(&error.to_string()))
        .filter(|message| !message.trim().is_empty())
        .collect::<Vec<_>>();
    let mut deduped = Vec::new();
    for part in parts {
        if deduped.last() == Some(&part) {
            continue;
        }
        deduped.push(part);
    }
    let mut summary = deduped.join(": ");
    if summary.chars().count() > PUBLIC_ERROR_CHAIN_MAX_CHARS {
        summary = summary.chars().take(PUBLIC_ERROR_CHAIN_MAX_CHARS).collect();
    }
    summary
}

pub(crate) fn sanitize_public_url(value: &str, non_http_label: &'static str) -> String {
    let value = value.trim();
    if !value.starts_with("http://") && !value.starts_with("https://") {
        return non_http_label.to_string();
    }
    let Ok(mut url) = reqwest::Url::parse(value) else {
        return PUBLIC_INVALID_URL_LABEL.to_string();
    };
    if !matches!(url.scheme(), "http" | "https") {
        return non_http_label.to_string();
    }
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    let sanitized = url.to_string();
    truncate_public_text(&sanitized, PUBLIC_URL_MAX_CHARS)
}

fn push_public_error_part<'a>(parts: &mut Vec<&'a str>, part: &'a str) {
    let duplicate_placeholder =
        matches!(part, "<redacted>" | "<path>") && parts.last() == Some(&part);
    if !duplicate_placeholder {
        parts.push(part);
    }
}

fn sanitize_public_error_part(part: &str) -> &str {
    let lower = part.to_ascii_lowercase();
    if contains_secret_marker(&lower) {
        "<redacted>"
    } else if is_path_like(part) {
        "<path>"
    } else {
        part
    }
}

fn secret_marker_requires_next_value(part: &str) -> bool {
    if part.contains('=') {
        return false;
    }
    let label = if let Some((label, value)) = part.split_once(':') {
        if !value.trim_matches(secret_value_boundary).is_empty() {
            return false;
        }
        label
    } else {
        part
    };
    let label = label.trim_matches(secret_value_boundary);
    let lower = label.to_ascii_lowercase();
    if lower.is_empty() {
        return false;
    }
    is_secret_label(&lower)
}

fn secret_value_boundary(ch: char) -> bool {
    matches!(ch, '"' | '\'' | '`' | ',' | ';')
}

fn is_secret_label(lower: &str) -> bool {
    [
        "token",
        "secret",
        "jwt",
        "key",
        "password",
        "passwd",
        "authorization",
        "bearer",
        "apikey",
        "api_key",
        "api-key",
    ]
    .contains(&lower)
        || ["_token", "_secret", "_jwt", "_key", "_password", "_passwd"]
            .iter()
            .any(|suffix| lower.ends_with(suffix))
}

fn contains_secret_marker(lower: &str) -> bool {
    [
        "token",
        "secret",
        "jwt",
        "key",
        "password",
        "passwd",
        "authorization",
        "bearer",
        "apikey",
        "api_key",
        "api-key",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

fn is_path_like(part: &str) -> bool {
    part.starts_with('/')
        || part.starts_with("file://")
        || part
            .split_once('=')
            .is_some_and(|(_, value)| value.starts_with('/') || value.starts_with("file://"))
        || part
            .split_once(':')
            .is_some_and(|(_, value)| value.starts_with('/') || value.starts_with("file://"))
}

fn truncate_public_text(value: &str, max_chars: usize) -> String {
    if value.chars().count() > max_chars {
        value.chars().take(max_chars).collect::<String>() + "..."
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_error_redacts_secret_words_and_paths() {
        let sanitized = sanitize_public_error(
            "spawn /Users/alice/bin/codex failed token abc Authorization=Bearer api_key=xyz cwd=/tmp/work file://secret",
        );

        assert!(!sanitized.contains("/Users/alice"));
        assert!(!sanitized.contains("/tmp/work"));
        assert!(!sanitized.contains("file://secret"));
        assert!(!sanitized.contains("token"));
        assert!(!sanitized.contains("Authorization"));
        assert!(!sanitized.contains("api_key"));
        assert!(!sanitized.contains(" abc "));
        assert!(sanitized.contains("<path>"));
        assert!(sanitized.contains("<redacted>"));
    }

    #[test]
    fn public_error_redacts_colon_secret_values() {
        let sanitized = sanitize_public_error(
            "authorization: Bearer raw-bearer token: raw-token api-key: raw-key password: raw-password OPENROUTER_API_KEY raw-env-key",
        );

        assert!(!sanitized.contains("raw-bearer"));
        assert!(!sanitized.contains("raw-token"));
        assert!(!sanitized.contains("raw-key"));
        assert!(!sanitized.contains("raw-password"));
        assert!(!sanitized.contains("raw-env-key"));
        assert_eq!(sanitized, "<redacted>");
    }

    #[test]
    fn public_error_chain_dedupes_and_truncates() {
        let error = anyhow::anyhow!("token abc").context("token abc");
        let sanitized = sanitize_public_error_chain(error.chain());

        assert_eq!(sanitized, "<redacted>");
    }

    #[test]
    fn public_url_removes_credentials_query_fragment_and_local_paths() {
        let sanitized = sanitize_public_url(
            "https://user:pass@example.test/daemon/releases/manifest.json?token=abc#secret",
            "<local>",
        );

        assert_eq!(
            sanitized,
            "https://example.test/daemon/releases/manifest.json"
        );
        assert_eq!(
            sanitize_public_url("file:///tmp/private", "<local>"),
            "<local>"
        );
        assert_eq!(
            sanitize_public_url("https://[invalid-url]?token=abc", "<local>"),
            "<invalid-url>"
        );
    }
}
