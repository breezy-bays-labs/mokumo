//! Re-exports `MokumoApp` from `mokumo_shop::graft`.
//!
//! The Graft impl moved to `crates/mokumo-shop/src/graft.rs` in PR 2.
//! This module re-exports for backward compatibility until services/api
//! is fully dissolved in PR 4.

pub use mokumo_shop::graft::MokumoApp;
