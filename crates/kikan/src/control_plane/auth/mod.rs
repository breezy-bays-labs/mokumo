//! Pure-function control-plane fns for the auth wire contract.
//!
//! Functions here are framework-agnostic: they take
//! [`crate::ControlPlaneState`] (or finer-grained slices), borrow a
//! database connection, and return plain Rust values plus typed
//! [`crate::ControlPlaneError`]s. The HTTP adapter at
//! [`crate::platform::v1::auth`] wraps each fn into an axum handler;
//! the UDS adapter (in `kikan-cli`) and any future transport reuse the
//! same fns without re-validating.
//!
//! Subject to the control-plane purity invariant — see
//! `tests/control_plane_purity.rs`. No `axum::*`, `tower::*`,
//! `http::*`, `axum_login::*`, or downstream-vertical imports may
//! appear in this tree. Lockout escalation, rate-limiting, and session
//! issuance stay in the adapter.
