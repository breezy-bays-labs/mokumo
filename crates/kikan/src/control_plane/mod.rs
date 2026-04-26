//! Pure-function control-plane layer for admin-surface business logic.
//!
//! `kikan::control_plane::*` is a transport-neutral surface callable from:
//! - HTTP Axum handlers (via `kikan::platform::*` thin delegations)
//! - Unix-domain-socket admin adapter (`kikan-admin-adapter` crate, PR-D)
//! - In-process CLI one-shot subcommands (`apps/mokumo-server` `bootstrap`,
//!   `diagnose`, `backup`)
//!
//! ## Purity invariant
//!
//! Code under `crates/kikan/src/control_plane/` must not import `axum::*`,
//! `tower::*`, `tower_sessions::*`, `tower_http::*`, `axum_login::*`,
//! `http::*`, or any downstream vertical-adapter crate. The regression
//! guard lives at `crates/kikan/tests/control_plane_purity.rs` and
//! carries the full list.
//!
//! Rationale: keeping control-plane fns free of transport machinery means a
//! single set of business-logic fns serves every admin caller without
//! re-wrapping or re-validating. The HTTP adapter owns cookie / session
//! / CSRF concerns; the UDS adapter owns capability auth via fs-perms; the
//! one-shot CLI owns tty prompts — none of which leak down into the
//! pure-fn layer.

pub mod auth;
pub mod profiles;
pub mod state;
pub mod types;
pub mod users;

pub use state::ControlPlaneState;
pub use types::{PinId, PinIdError, SetupTokenSource};
