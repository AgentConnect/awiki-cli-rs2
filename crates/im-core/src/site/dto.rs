use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SiteDomain(String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SiteRootDraft {
    pub domain: SiteDomain,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SiteRootDocument {
    pub domain: SiteDomain,
    pub body: Option<String>,
    pub raw: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SitePageRef {
    pub domain: SiteDomain,
    pub slug: crate::content::PageSlug,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SitePageQuery {
    pub domain: SiteDomain,
    pub limit: crate::ids::PageLimit,
    pub cursor: Option<crate::ids::Cursor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SitePageDraft {
    pub domain: SiteDomain,
    pub slug: crate::content::PageSlug,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SitePageUpdate {
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SitePageDocument {
    pub domain: SiteDomain,
    pub slug: crate::content::PageSlug,
    pub body: Option<String>,
    pub raw: serde_json::Value,
}

impl SiteDomain {
    pub fn parse(input: impl AsRef<str>) -> crate::ImResult<Self> {
        let normalized = input
            .as_ref()
            .trim()
            .to_ascii_lowercase()
            .trim_end_matches('.')
            .to_string();
        if normalized.is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("domain".to_string()),
                "did_domain is required",
            ));
        }
        if normalized.contains("://") {
            return Err(crate::ImError::invalid_input(
                Some("domain".to_string()),
                "did_domain must be a bare domain without a URL scheme",
            ));
        }
        if normalized.contains(['/', '?', '#']) {
            return Err(crate::ImError::invalid_input(
                Some("domain".to_string()),
                "did_domain must not include a path, query, or fragment",
            ));
        }
        if normalized.contains(':') {
            return Err(crate::ImError::invalid_input(
                Some("domain".to_string()),
                "did_domain must not include a port",
            ));
        }
        if normalized.chars().any(char::is_whitespace) {
            return Err(crate::ImError::invalid_input(
                Some("domain".to_string()),
                "did_domain must not contain whitespace",
            ));
        }
        if normalized.contains('@') || normalized.contains('%') {
            return Err(crate::ImError::invalid_input(
                Some("domain".to_string()),
                "did_domain must be a bare domain",
            ));
        }
        Ok(Self(normalized))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl SitePageRef {
    pub fn new(domain: SiteDomain, slug: crate::content::PageSlug) -> Self {
        Self { domain, slug }
    }
}

impl Default for SitePageQuery {
    fn default() -> Self {
        Self {
            domain: SiteDomain(String::new()),
            limit: crate::ids::PageLimit(50),
            cursor: None,
        }
    }
}
