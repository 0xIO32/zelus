// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]

mod key;
#[cfg(feature = "server")]
pub mod server;

pub use key::*;

pub const ROOT_KEY_COMMON_NAME: &str = "Novium CA";
pub const BACKEND_KEY_COMMON_NAME: &str = "Novium Backend";
