//! HTTP application error — re-exported from `kikan::app_error`.
//!
//! The canonical `AppError` type lives in the kikan platform crate
//! so kikan-owned handlers (diagnostics, demo reset, backup status,
//! etc.) can return `Result<_, AppError>` without depending on
//! `services/api`. `services/api` re-exports it here to keep the
//! existing `crate::error::AppError` import paths working.
//!
//! Adding new variants: if the variant is HTTP-generic (status code plus
//! `ErrorCode`), add it in `kikan::app_error`. Shop-vertical error shapes —
//! if they ever need a dedicated variant — should live in a vertical crate
//! (e.g. `mokumo-shop`) and map through an existing `AppError` variant, not
//! inside `AppError` itself.

pub use kikan::AppError;
