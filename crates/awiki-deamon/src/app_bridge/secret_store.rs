#[derive(Clone, PartialEq, Eq)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for SecretString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted-secret>")
    }
}

impl std::fmt::Display for SecretString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted-secret>")
    }
}

pub fn secret_from_private_key_multibase(value: &str) -> SecretString {
    SecretString::new(value.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_debug_and_display_redact_value() {
        let secret = SecretString::new("z-private-key-material");
        assert_eq!(secret.expose_secret(), "z-private-key-material");
        assert!(!format!("{secret:?}").contains("private-key-material"));
        assert!(!format!("{secret}").contains("private-key-material"));
    }
}
