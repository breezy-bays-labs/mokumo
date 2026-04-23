//! SvelteKit SPA serving — `kikan::data_plane::spa::SpaSource` implementations.
//!
//! Two variants:
//!
//! - [`SvelteKitSpa`] — embedded build via [`rust_embed::RustEmbed`]. One
//!   binary, no disk reads at runtime. Desktop consumers embed
//!   `apps/web/build` this way.
//! - [`SvelteKitSpaDir`] — disk build served from a runtime directory. For
//!   headless / containerized deployments that ship the SPA separately.
//!
//! Both apply SvelteKit's cache policy: `_app/immutable/*` assets get a
//! one-year immutable cache; every other asset (including `index.html`)
//! gets a one-hour cache with `no-cache` on the SPA shell itself. `/api/**`
//! routing belongs to the outer router — the fallback never sees API
//! paths because API routes register first in [`kikan::Engine::build_router`].

mod disk;
mod embedded;

pub use disk::SvelteKitSpaDir;
pub use embedded::SvelteKitSpa;

/// SvelteKit `adapter-static` publishes immutable build artifacts under
/// `_app/immutable/`; those get a one-year cache. Everything else (the
/// `index.html` shell, `favicon.ico`, static assets under `static/`) gets
/// a one-hour cache — short enough that shops pick up updated builds on
/// their next browser reload without per-request round-trips.
///
/// The path argument is already stripped of any leading `/`, so the
/// prefix match is against `_app/immutable/…`, not `/_app/immutable/…`.
pub(crate) fn cache_policy_for(path: &str) -> &'static str {
    if path.starts_with("_app/immutable/") {
        "public, max-age=31536000, immutable"
    } else {
        "public, max-age=3600"
    }
}

#[cfg(test)]
mod tests {
    use super::cache_policy_for;

    #[test]
    fn cache_policy_long_for_immutable_assets() {
        assert_eq!(
            cache_policy_for("_app/immutable/chunks/app.js"),
            "public, max-age=31536000, immutable",
        );
    }

    #[test]
    fn cache_policy_short_for_mutable_assets() {
        assert_eq!(cache_policy_for("favicon.ico"), "public, max-age=3600");
        assert_eq!(cache_policy_for("index.html"), "public, max-age=3600");
        // Regression guard: an earlier implementation matched against
        // `/_app/immutable/` AFTER the caller stripped the leading slash,
        // silently demoting every immutable asset to the 1h cache. The
        // prefix must work against the stripped form.
        assert_eq!(
            cache_policy_for("app/_app/immutable/x.js"),
            "public, max-age=3600",
        );
    }
}
