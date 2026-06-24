use anyhow::Chain;

const PUBLIC_ERROR_MAX_CHARS: usize = 240;
const PUBLIC_ERROR_CHAIN_MAX_CHARS: usize = 360;

pub(crate) fn sanitize_public_error(message: &str) -> String {
    let mut redact_next = false;
    let mut parts = Vec::new();
    for part in message.split_whitespace() {
        if redact_next {
            push_public_error_part(&mut parts, "<redacted>");
            redact_next = false;
            continue;
        }
        let sanitized = sanitize_public_error_part(part);
        if sanitized == "<redacted>" && !part.contains('=') && !part.contains(':') {
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
    fn public_error_chain_dedupes_and_truncates() {
        let error = anyhow::anyhow!("token abc").context("token abc");
        let sanitized = sanitize_public_error_chain(error.chain());

        assert_eq!(sanitized, "<redacted>");
    }
}
