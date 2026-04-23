//! Disk-served SvelteKit SPA — reads assets from a runtime directory.

use std::path::PathBuf;

use axum::Router;
use axum::extract::Request;
use axum::http::{HeaderValue, header};
use axum::middleware::{self, Next};
use axum::response::Response;
use kikan::data_plane::spa::SpaSource;
use tower_http::services::{ServeDir, ServeFile};

use crate::cache_policy_for;

/// Serves a SvelteKit build from an on-disk directory.
///
/// `dir` must contain an `index.html` at its root — headless consumers
/// (e.g. `mokumo-server --spa-dir`) validate this at boot. Missing
/// assets fall back to `index.html` so SvelteKit's client-side router
/// handles deep links.
///
/// Cache headers are stamped by a response middleware
/// ([`apply_sveltekit_cache_headers`]) because [`ServeDir`] does not set
/// `Cache-Control` on its own.
pub struct SvelteKitSpaDir {
    pub dir: PathBuf,
}

impl SvelteKitSpaDir {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }
}

impl SpaSource for SvelteKitSpaDir {
    fn router(&self) -> Router {
        let index = self.dir.join("index.html");
        let serve_dir = ServeDir::new(&self.dir).fallback(ServeFile::new(&index));

        Router::new()
            .fallback_service(serve_dir)
            .layer(middleware::from_fn(apply_sveltekit_cache_headers))
    }
}

/// Stamps `Cache-Control` on SPA responses.
///
/// Decision order matters here: HTML responses are detected by
/// `Content-Type` *before* any path-based match, because `ServeDir`
/// falls back to `index.html` when an asset is missing — so a request
/// for `/_app/immutable/missing.js` that fell back to the shell would
/// otherwise be tagged with the 1-year immutable cache. Classifying by
/// the rendered body first protects against that.
///
/// - 404 (missing file that never resolved to the shell) → `no-store`.
/// - Non-2xx other than 404 (5xx, 405, …) → `no-store`; pinning a
///   transient error response would outlive the cause.
/// - HTML response body → `no-cache`; the SvelteKit shell must refetch
///   so shops pick up new builds on reload.
/// - Path under `_app/immutable/*` with a 2xx non-HTML body → 1-year
///   immutable (fingerprinted asset, safe to pin).
/// - Everything else with a 2xx body → 1-hour public cache.
async fn apply_sveltekit_cache_headers(req: Request, next: Next) -> Response {
    let request_path = req.uri().path().trim_start_matches('/').to_owned();
    let response = next.run(req).await;

    let (mut parts, body) = response.into_parts();

    let cache = if !parts.status.is_success() {
        // 404 / 5xx / 4xx all collapse to `no-store`: transient errors
        // must not be pinned by intermediaries. A 200 is required for
        // anything longer-lived.
        "no-store"
    } else if is_html_response(&parts) {
        // Evaluated before the path-based check so a request for a
        // missing fingerprinted asset that fell back to the shell
        // doesn't inherit the 1-year immutable policy.
        "no-cache"
    } else {
        cache_policy_for(&request_path)
    };

    parts
        .headers
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));

    Response::from_parts(parts, body)
}

fn is_html_response(parts: &axum::http::response::Parts) -> bool {
    parts
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("text/html"))
}
