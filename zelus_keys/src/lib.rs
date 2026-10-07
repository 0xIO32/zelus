// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]

mod key;
#[cfg(feature = "server")]
pub mod server;
#[cfg(feature = "setup")]
pub mod setup;

pub use key::*;
