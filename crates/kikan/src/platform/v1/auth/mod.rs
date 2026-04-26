//! Auth handlers under `/api/platform/v1/auth/*`.
//!
//! Thin axum adapters wrapping [`crate::control_plane::auth`]. The
//! adapters own session/cookie issuance, rate-limit + lockout
//! enforcement, and CSRF/Origin checks; the pure-fn layer owns
//! credential verification and persistence.
