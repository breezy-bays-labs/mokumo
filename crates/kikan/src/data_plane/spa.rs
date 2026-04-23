//! SPA (single-page-application) serving seam.
//!
//! The data-plane fallback is how the Engine serves a browser SPA — in
//! Mokumo's case, the SvelteKit build from `apps/web/build`. Kikan owns
//! the *composition point* (API routes register first, SPA serves
//! everything else); the actual asset bytes live in a sister crate that
//! picks the embedding strategy.
//!
//! `kikan` stays rust-embed-free — pulling a build-artifact-dependent
//! crate into every kikan build would violate invariant I5. The
//! sister-crate pattern (`kikan-spa-sveltekit`, any future `kikan-spa-*`)
//! lets consumers opt in at the edge.
//!
//! # Usage
//!
//! A [`Graft`](crate::Graft) may return `Some(Box<dyn SpaSource>)` from
//! [`Graft::spa_source`](crate::Graft::spa_source). [`Engine::new_with`]
//! captures it once at construction and mounts the returned router as
//! the data-plane fallback inside
//! [`Engine::build_router`](crate::Engine::build_router) —
//! `API routes register first, fallback last`, which is idiomatic Axum.
//!
//! Grafts that don't serve an SPA (headless deployments, CLI-only tools,
//! tests) return `None`; the engine skips fallback registration and
//! non-API paths produce Axum's default 404.

/// A source of SPA assets, rendered as an [`axum::Router`].
///
/// Returning a `Router` (rather than a `tower::Service` or a bare handler
/// function) keeps the composition point aligned with Axum idiom: the
/// consumer router calls `.fallback_service(source.router().into_service())`
/// and the SPA inherits the outer router's layers, extractors, and error
/// handling without adapter plumbing.
///
/// Implementors are consumed as `Box<dyn SpaSource>` — the `Send + Sync +
/// 'static` bounds permit the box to live on [`Engine`](crate::Engine) and
/// be referenced across tasks at router-build time. Capability-via-data:
/// kikan never matches on concrete variants.
pub trait SpaSource: Send + Sync + 'static {
    /// Return an [`axum::Router`] that serves the SPA.
    ///
    /// Consumers mount the returned router as the data-plane's fallback
    /// service. API routes register first, so this router never sees
    /// `/api/**` requests and doesn't need to filter them out.
    ///
    /// Called once, at router-build time in
    /// [`Engine::build_router`](crate::Engine::build_router). Not a
    /// per-request hot path.
    fn router(&self) -> axum::Router;
}
