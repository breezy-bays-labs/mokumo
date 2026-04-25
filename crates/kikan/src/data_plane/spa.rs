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

/// A prefix-scoped mount inside a [`CompositeSpaSource`].
///
/// Mounts let one composed origin serve multiple co-existing SPAs: the
/// shop SPA at `/` and the platform admin UI at `/admin/*`. Per-extension
/// subtrees under `/admin/extensions/{ext_id}` route back to the shop
/// SPA because an extension's detail UI is a SubGraft on the shop data
/// plane, not part of the platform admin surface.
pub struct SpaMount {
    prefix: String,
    source: Box<dyn SpaSource>,
}

impl SpaMount {
    /// Construct a mount. The `prefix` must start with `/` and must not
    /// end with `/` — except for the reserved root `/`, which should not
    /// be registered as a mount (pass the root source as the fallback).
    pub fn new(prefix: impl Into<String>, source: Box<dyn SpaSource>) -> Self {
        let prefix = prefix.into();
        debug_assert!(
            prefix.starts_with('/'),
            "SpaMount prefix must start with /: got {prefix:?}"
        );
        debug_assert!(
            !prefix.ends_with('/') || prefix.len() == 1,
            "SpaMount prefix must not end with / unless it is the root: got {prefix:?}"
        );
        Self { prefix, source }
    }
}

/// A composed [`SpaSource`] that dispatches prefix-scoped mounts to nested
/// SPA sources, with a fallback for unmatched paths.
///
/// Dispatch is longest-prefix-first. Mounts are nested via
/// [`axum::Router::nest`], which strips the matched prefix before
/// invoking the nested router — so a request for
/// `/admin/_app/immutable/chunks/app.js` sent to the `/admin`-mounted
/// admin SPA sees `/_app/immutable/chunks/app.js`, which matches what
/// the SvelteKit `adapter-static` build with `kit.paths.base = "/admin"`
/// emitted into its rust-embed bundle.
///
/// Prefix uniqueness is `debug_assert!`-enforced at construction; duplicate
/// prefixes indicate a composition bug and fail loudly in debug builds.
///
/// The composite is itself a [`SpaSource`], so it composes transparently
/// into existing engine build-router machinery — no dedicated registration
/// path is needed on the engine.
///
/// # Trailing-slash normalization
///
/// Axum's `.nest("/admin", ...)` matches `/admin` exact and
/// `/admin/<non-empty-tail>`, but does **not** match the bare-trailing-slash
/// form `/admin/`. Consumers that want `/admin/` to reach the admin SPA
/// (typical browser behavior) should wrap the final composed router with
/// [`tower_http::normalize_path::NormalizePathLayer::trim_trailing_slash`]
/// at the service level — it rewrites the request URI before route
/// matching, where a router-level `.layer` cannot reach. The layer is
/// idempotent for paths that don't end in `/`, so applying it globally
/// has no side effects on other routes.
pub struct CompositeSpaSource {
    fallback: Box<dyn SpaSource>,
    mounts: Vec<SpaMount>,
}

impl CompositeSpaSource {
    /// Construct with a fallback source. Requests that don't match any
    /// registered mount prefix dispatch here.
    pub fn new(fallback: Box<dyn SpaSource>) -> Self {
        Self {
            fallback,
            mounts: Vec::new(),
        }
    }

    /// Register a prefix-scoped mount.
    ///
    /// In debug builds this panics if the same prefix is registered twice
    /// — that's a composition bug, not a runtime condition.
    pub fn with_mount(mut self, prefix: impl Into<String>, source: Box<dyn SpaSource>) -> Self {
        let mount = SpaMount::new(prefix, source);
        debug_assert!(
            !self.mounts.iter().any(|m| m.prefix == mount.prefix),
            "duplicate SpaMount prefix: {:?}",
            mount.prefix
        );
        self.mounts.push(mount);
        self
    }

    /// Return registered prefixes sorted longest-first.
    ///
    /// Exposed for the platform self-check probe surface — Diagnostics
    /// reads this to render the live dispatch configuration.
    pub fn dispatch_summary(&self) -> Vec<String> {
        let mut prefixes: Vec<String> = self.mounts.iter().map(|m| m.prefix.clone()).collect();
        prefixes.sort_by_key(|p| std::cmp::Reverse(p.len()));
        prefixes
    }
}

impl SpaSource for CompositeSpaSource {
    fn router(&self) -> axum::Router {
        let mut router = axum::Router::new();
        // Nest longer prefixes first so `/admin/extensions/{id}` registers
        // before `/admin`. Axum route matching is specificity-first within
        // a router, so order between non-overlapping nests is cosmetic —
        // but registering longest-first matches the mental model.
        let mut sorted: Vec<&SpaMount> = self.mounts.iter().collect();
        sorted.sort_by_key(|m| std::cmp::Reverse(m.prefix.len()));
        for mount in sorted {
            router = router.nest(&mount.prefix, mount.source.router());
        }
        router.fallback_service(self.fallback.router())
    }
}
