#![allow(dead_code)]

pub(crate) mod fake_provider;
pub(crate) mod incoming;
pub(crate) mod lifecycle;
pub(crate) mod native_provider;
pub(crate) mod notices;
pub(crate) mod provider;
pub(crate) mod repair;
pub(crate) mod runtime;
pub(crate) mod state_ref;
pub(crate) mod status;
pub(crate) mod storage;
pub(crate) mod summary;
pub(crate) mod v2_application;
pub(crate) mod v2_lifecycle;
pub(crate) mod v2_notice;
pub(crate) mod v2_product;
pub(crate) mod v2_runtime;
pub(crate) mod v2_status;
pub(crate) mod wire;

pub(crate) const DEFAULT_GROUP_MLS_DEVICE_ID: &str = "default";
