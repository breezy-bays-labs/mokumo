//! Embedded SvelteKit SPA — [`rust_embed::RustEmbed`]-backed [`SpaSource`].

use std::marker::PhantomData;

use axum::Router;
use axum::http::{StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use kikan::data_plane::spa::SpaSource;
use rust_embed::RustEmbed;

use crate::cache_policy_for;

/// Serves a [`RustEmbed`]-generated asset bundle as a SvelteKit SPA.
///
/// The generic `A` is a marker type produced by `#[derive(rust_embed::Embed)]`
/// at the consumer's crate — the derive encodes the asset directory via a
/// `#[folder = "..."]` attribute that's resolved at the consumer's compile
/// time. `SvelteKitSpa` itself holds no data; `PhantomData<A>` keeps the
/// type parameter live without forcing the consumer to instantiate `A`.
///
/// Missing assets fall back to `index.html` so SvelteKit's client-side
/// router can handle deep links (e.g. a cold reload of `/customers/42`
/// serves the SPA shell rather than a 404). If the embedded bundle is
/// missing `index.html` entirely the fallback returns a 404 with a
/// descriptive body — that case means the consumer built the binary
/// without first running the SPA build step.
pub struct SvelteKitSpa<A: RustEmbed> {
    _marker: PhantomData<A>,
}

impl<A: RustEmbed> SvelteKitSpa<A> {
    /// Construct a new embedded SPA source. `A::get(path)` is invoked on
    /// every fallback request; `rust-embed` generates a static table at
    /// compile time so lookups are allocation-free after construction.
    pub fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

impl<A: RustEmbed> Default for SvelteKitSpa<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: RustEmbed + Send + Sync + 'static> SpaSource for SvelteKitSpa<A> {
    fn router(&self) -> Router {
        Router::new().fallback(serve_embedded::<A>)
    }
}

async fn serve_embedded<A: RustEmbed>(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    if let Some(file) = A::get(path) {
        // A direct hit on the HTML shell (e.g. `/index.html`) must not
        // inherit the 1-hour cache; a pinned shell would prevent shops
        // from picking up rebuilt SPAs on reload. Gate `no-cache` on
        // content-type so we don't need to enumerate filenames.
        let mime = file.metadata.mimetype();
        let cache = if mime.starts_with("text/html") {
            "no-cache"
        } else {
            cache_policy_for(path)
        };
        return spa_response(StatusCode::OK, mime, cache, file.data.to_vec());
    }

    if let Some(index) = A::get("index.html") {
        // SPA shell fallback for a client-side route — also no-cache.
        return spa_response(
            StatusCode::OK,
            index.metadata.mimetype(),
            "no-cache",
            index.data.to_vec(),
        );
    }

    // Missing `index.html` means the embedded bundle was built without
    // the SvelteKit output — a build-ordering bug, not a user error.
    // Surface it loudly rather than returning an empty 404.
    tracing::error!(
        "SvelteKit SPA bundle is missing index.html — did the SPA build run before the Rust build?"
    );
    spa_response(
        StatusCode::NOT_FOUND,
        "text/plain",
        "no-store",
        b"SPA not built. Run: moon run web:build".to_vec(),
    )
}

fn spa_response(status: StatusCode, content_type: &str, cache: &str, body: Vec<u8>) -> Response {
    (
        status,
        [
            (axum::http::header::CONTENT_TYPE, content_type.to_owned()),
            (axum::http::header::CACHE_CONTROL, cache.to_owned()),
        ],
        body,
    )
        .into_response()
}
