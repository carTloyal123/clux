//! The end-to-end test harness.
//!
//! The `pub use` globs below re-export helpers that the integration test binaries
//! (`tests/*.rs`) rely on through `use common::harness::*`. They look unused from
//! within this crate's own compilation, so `cargo fix` will try to remove them -
//! the `#![allow(unused_imports)]` keeps that from breaking the test binaries.
#![allow(unused_imports)]

mod asserts;
mod client;
mod helpers;
mod queries;
mod types;

pub use asserts::*;
pub use client::*;
pub use helpers::*;
pub use types::*;
