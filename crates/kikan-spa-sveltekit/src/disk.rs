//! Disk-served SvelteKit SPA — reads assets from a runtime directory.

use std::path::PathBuf;

use axum::Router;
use axum::extract::Request;
use axum::http::{HeaderValue, StatusCode, header};
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

/// Stamps `Cache-Control` on SPA responses:
///
/// - `_app/immutable/*` → 1-year immutable (fingerprinted, safe to pin).
/// - HTML responses (the SvelteKit shell, served for deep links) →
///   `no-cache` so shops pick up new builds on reload.
/// - Non-HTML, non-immutable assets → 1-hour public cache.
/// - Any 404 (e.g. a file that existed at boot but was removed at
///   runtime) → `no-store`, since caching a transient 404 would outlive
///   the underlying cause.
async fn apply_sveltekit_cache_headers(req: Request, next: Next) -> Response {
    let request_path = req.uri().path().trim_start_matches('/').to_owned();
    let response = next.run(req).await;

    let (mut parts, body) = response.into_parts();

    let cache = if parts.status == StatusCode::NOT_FOUND {
        "no-store"
    } else if request_path.starts_with("_app/immutable/") {
        cache_policy_for(&request_path)
    } else {
        // When ServeDir falls back to `index.html`, the response is the
        // SPA shell for a client-side route — the request path looks like
        // an app route (`/customers/42`) but the body is HTML. Detect
        // that by content-type so the shell never pins to an hour.
        let is_html = parts
            .headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("text/html"));
        if is_html {
            "no-cache"
        } else {
            cache_policy_for(&request_path)
        }
    };

    parts
        .headers
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));

    Response::from_parts(parts, body)
}
