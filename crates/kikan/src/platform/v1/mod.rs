//! `/api/platform/v1/*` — the kikan platform API namespace.
//!
//! Routers under this tree mount at the kikan-canonical
//! `/api/platform/v1/...` prefix in [`crate::Engine::build_router`].
//! Handlers are thin axum adapters delegating to the transport-neutral
//! fns in [`crate::control_plane`] and rendering responses in the
//! standard kikan error contract ([`crate::AppError`]).

pub mod auth;
