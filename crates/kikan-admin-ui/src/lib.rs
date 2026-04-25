//! Kikan admin UI sister crate.
//!
//! Owns the rust-embed asset bundle for the platform admin surface and
//! exposes a factory that returns a [`SpaSource`] backed by that bundle.
//! The composition mount point (`/admin` on the composed Tauri origin)
//! and the dispatch logic live upstream in `kikan::data_plane::spa`
//! (see `CompositeSpaSource`); this crate only holds asset bytes.
//!
//! The `#[folder]` path resolves at this crate's compile time — the
//! frontend build must therefore produce `frontend/build/` before this
//! crate's Rust build runs. Moon declares the dependency so
//! `kikan-admin-ui:build` depends on `kikan-admin-ui:frontend-build`.
//!
//! Reusing `kikan_spa_sveltekit::SvelteKitSpa<A: RustEmbed>` keeps the
//! sister-crate pattern intact: no new `SpaSource` implementation, no
//! rust-embed coupling into `kikan`.
//!
//! [`SpaSource`]: kikan::data_plane::spa::SpaSource

use kikan::data_plane::spa::SpaSource;
use kikan_spa_sveltekit::SvelteKitSpa;

/// Rust-embed marker for the admin UI asset bundle.
///
/// The folder path is relative to this crate's `Cargo.toml`. `rust-embed`
/// emits a compile-time error if the directory is absent, which is the
/// loud-fail we want when the frontend build hasn't run.
#[derive(rust_embed::Embed)]
#[folder = "frontend/build"]
pub struct KikanAdminUiAssets;

/// Construct the admin UI's SPA source.
///
/// Returns `Box<dyn SpaSource>` so it can be handed directly to
/// `CompositeSpaSource::with_mount` without the caller needing to name
/// the concrete type.
pub fn admin_spa_source() -> Box<dyn SpaSource> {
    Box::new(SvelteKitSpa::<KikanAdminUiAssets>::new())
}
