pub(super) fn complete_bare_handle(target: &str, did_domain: &str) -> String {
    let trimmed = target.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let lower = trimmed.to_lowercase();
    if lower.starts_with("did:") {
        return trimmed.to_string();
    }
    let handle = lower.strip_prefix("wba://").unwrap_or(&lower);
    if handle.contains('.') {
        return trimmed.to_string();
    }
    let domain = normalize_handle_domain(did_domain);
    if domain.is_empty() {
        return trimmed.to_string();
    }
    format!("{handle}.{domain}")
}

fn normalize_handle_domain(domain: &str) -> String {
    let value = domain.trim().to_lowercase();
    value
        .strip_suffix('.')
        .map(ToString::to_string)
        .unwrap_or(value)
}

#[cfg(test)]
mod tests {
    #[test]
    fn complete_bare_handle_matches_legacy_display_boundaries() {
        assert_eq!(
            super::complete_bare_handle("Alice", "Tenant.Example."),
            "alice.tenant.example"
        );
        assert_eq!(
            super::complete_bare_handle("wba://Alice", "Tenant.Example."),
            "alice.tenant.example"
        );
        assert_eq!(
            super::complete_bare_handle("Alice.Other.Example", "Tenant.Example."),
            "Alice.Other.Example"
        );
        assert_eq!(
            super::complete_bare_handle("did:wba:tenant.example:user:alice:e1", "x"),
            "did:wba:tenant.example:user:alice:e1"
        );
    }
}
