#![allow(dead_code)]
#![allow(clippy::too_many_arguments)]
#![allow(
    clippy::large_enum_variant,
    reason = "FRB DTO enum layouts are part of the generated Dart bridge contract"
)]
#![allow(
    clippy::result_large_err,
    reason = "DartImError is an FRB value DTO; boxing it would change the generated bridge API"
)]

pub mod api;
pub mod dto;
pub mod mapping;

#[allow(clippy::all)]
#[allow(unused)]
pub mod frb_generated;
