pub const IM_CORE_MVP_ENV: &str = "AWIKI_USE_IM_CORE_MVP";

pub fn use_im_core_mvp() -> bool {
    std::env::var(IM_CORE_MVP_ENV)
        .ok()
        .is_some_and(|value| env_flag_enabled(&value))
}

pub(crate) fn env_flag_enabled(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}
