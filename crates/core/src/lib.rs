//! `mokumo-core` (crate `mokumo-core`, directory `crates/core/`) —
//! shared low-level primitives consumed by both `kikan` and
//! `mokumo-shop`.
//!
//! Holds the actor-id newtype, activity-log entry shape, generic
//! filter/pagination helpers, error scaffolding, and setup-time
//! types. No workspace dependencies. Add code here only when at
//! least two crates need it AND it has no platform-state or
//! shop-vertical semantics — otherwise it belongs in `kikan` or
//! `mokumo-shop`.

pub mod activity;
pub mod actor;
pub mod error;
pub mod filter;
pub mod pagination;
pub mod setup;
