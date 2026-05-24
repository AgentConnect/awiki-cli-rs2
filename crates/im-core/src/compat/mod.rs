//! Migration-only compatibility surface.
//!
//! This module exists to keep the CLI cutover and legacy contract tests moving
//! while high-level `im-core` services absorb the remaining bridge behavior. It
//! is intentionally outside the stable SDK surface: do not re-export it from
//! `prelude`, do not mirror it into Dart facade models, and do not rely on it
//! for semver-stable application code.

pub mod attachments;
pub mod directory;
pub mod email;
pub mod groups;
pub mod identity;
pub mod local_state;
pub mod messages;
pub mod profile;
pub mod proof;
pub mod realtime;
pub mod wire;
