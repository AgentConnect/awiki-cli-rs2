pub(crate) fn profile_from_summary(summary: &super::IdentitySummary) -> super::Profile {
    super::Profile {
        subject: summary.did.clone(),
        handle: summary.handle.clone(),
        display_name: summary.display_name.clone(),
        bio: None,
        tags: Vec::new(),
        markdown: None,
        avatar_url: None,
        updated_at: None,
        metadata: Vec::new(),
    }
}

pub(crate) fn validate_profile_patch(patch: &super::ProfilePatch) -> crate::ImResult<()> {
    if patch
        .display_name
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(crate::ImError::invalid_input(
            Some("display_name".to_string()),
            "display name must not be empty when provided",
        ));
    }
    if patch
        .tags
        .as_ref()
        .is_some_and(|tags| tags.iter().any(|tag| tag.trim().is_empty()))
    {
        return Err(crate::ImError::invalid_input(
            Some("tags".to_string()),
            "tags must not contain empty values",
        ));
    }
    Ok(())
}
