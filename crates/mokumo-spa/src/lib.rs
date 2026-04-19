//! Embedded SvelteKit SPA, served as an Axum fallback.
//!
//! Consumers (e.g. `mokumo-desktop`) mount `serve_spa` via
//! `Router::fallback(mokumo_spa::serve_spa)`. Headless deployments
//! (`mokumo-server`) don't depend on this crate.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "../../apps/web/build"]
pub struct SpaAssets;

/// SPA fallback: serve embedded static assets or `index.html` for
/// client-side routing. `/api/**` paths return a JSON 404 instead of
/// the SPA shell so missing routes produce a typed error contract
/// rather than HTML.
pub async fn serve_spa(uri: axum::http::Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    if path == "api" || path.starts_with("api/") {
        let body = kikan_types::error::ErrorBody {
            code: kikan_types::error::ErrorCode::NotFound,
            message: "No API route matches this path".into(),
            details: None,
        };
        return (
            StatusCode::NOT_FOUND,
            [(axum::http::header::CACHE_CONTROL, "no-store")],
            Json(body),
        )
            .into_response();
    }

    if let Some(file) = SpaAssets::get(path) {
        let cache = if path.contains("/_app/immutable/") {
            "public, max-age=31536000, immutable"
        } else {
            "public, max-age=3600"
        };
        spa_response(
            StatusCode::OK,
            file.metadata.mimetype(),
            cache,
            file.data.to_vec(),
        )
    } else if let Some(index) = SpaAssets::get("index.html") {
        spa_response(
            StatusCode::OK,
            index.metadata.mimetype(),
            "no-cache",
            index.data.to_vec(),
        )
    } else {
        tracing::warn!("SPA assets not found — run: moon run web:build");
        spa_response(
            StatusCode::NOT_FOUND,
            "text/plain",
            "no-store",
            b"SPA not built. Run: moon run web:build".to_vec(),
        )
    }
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
