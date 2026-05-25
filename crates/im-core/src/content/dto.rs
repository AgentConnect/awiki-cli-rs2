use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PageSlug(String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageRef {
    pub slug: PageSlug,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Public,
    Draft,
    Unlisted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageDraft {
    pub slug: PageSlug,
    pub title: String,
    pub body: String,
    pub visibility: Visibility,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PageUpdate {
    pub title: Option<String>,
    pub body: Option<String>,
    pub visibility: Option<Visibility>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentPageQuery {
    pub limit: crate::ids::PageLimit,
    pub cursor: Option<crate::ids::Cursor>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageDocument {
    pub slug: PageSlug,
    pub title: Option<String>,
    pub body: Option<String>,
    pub visibility: Option<Visibility>,
    pub raw: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageDeleteResult {
    pub deleted: bool,
    pub raw: serde_json::Value,
}

impl PageSlug {
    pub fn parse(input: impl AsRef<str>) -> crate::ImResult<Self> {
        let value = input.as_ref().trim();
        if value.is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("slug".to_string()),
                "slug is required",
            ));
        }
        Ok(Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl PageRef {
    pub fn new(slug: PageSlug) -> Self {
        Self { slug }
    }
}

impl Visibility {
    pub fn parse(input: impl AsRef<str>) -> crate::ImResult<Self> {
        match input.as_ref().trim().to_ascii_lowercase().as_str() {
            "" | "public" => Ok(Self::Public),
            "draft" => Ok(Self::Draft),
            "unlisted" => Ok(Self::Unlisted),
            _ => Err(crate::ImError::invalid_input(
                Some("visibility".to_string()),
                "visibility must be one of public, draft, or unlisted",
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Draft => "draft",
            Self::Unlisted => "unlisted",
        }
    }
}

impl PageDraft {
    pub fn new(
        slug: PageSlug,
        title: impl Into<String>,
        body: impl Into<String>,
        visibility: Visibility,
    ) -> crate::ImResult<Self> {
        let title = title.into();
        if title.trim().is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("title".to_string()),
                "title is required",
            ));
        }
        Ok(Self {
            slug,
            title,
            body: body.into(),
            visibility,
        })
    }
}

impl Default for ContentPageQuery {
    fn default() -> Self {
        Self {
            limit: crate::ids::PageLimit(50),
            cursor: None,
        }
    }
}
