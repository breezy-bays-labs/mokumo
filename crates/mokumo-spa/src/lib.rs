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

    if is_api_path(path) {
        return api_not_found();
    }

    if let Some(file) = SpaAssets::get(path) {
        spa_response(
            StatusCode::OK,
            file.metadata.mimetype(),
            cache_policy_for(path),
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

/// `/api/**` paths are API surface, not SPA routes — return a typed 404
/// envelope instead of the SPA shell so clients see the same error
/// contract as any other missing API route.
fn is_api_path(path: &str) -> bool {
    path == "api" || path.starts_with("api/")
}

/// The path arg is post-`trim_start_matches('/')`, so SvelteKit assets
/// appear as `_app/immutable/...` — no leading slash.
fn cache_policy_for(path: &str) -> &'static str {
    if path.starts_with("_app/immutable/") {
        "public, max-age=31536000, immutable"
    } else {
        "public, max-age=3600"
    }
}

fn api_not_found() -> Response {
    let body = kikan_types::error::ErrorBody {
        code: kikan_types::error::ErrorCode::NotFound,
        message: "No API route matches this path".into(),
        details: None,
    };
    (
        StatusCode::NOT_FOUND,
        [(axum::http::header::CACHE_CONTROL, "no-store")],
        Json(body),
    )
        .into_response()
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::Uri;

    #[test]
    fn is_api_path_matches_exact_and_prefix() {
        assert!(is_api_path("api"));
        assert!(is_api_path("api/customers"));
        assert!(is_api_path("api/v1/nested/resource"));
    }

    #[test]
    fn is_api_path_rejects_lookalikes() {
        assert!(!is_api_path(""));
        assert!(!is_api_path("apiv2/x"));
        assert!(!is_api_path("api-docs"));
        assert!(!is_api_path("favicon.ico"));
        assert!(!is_api_path("_app/immutable/chunk.js"));
    }

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
        // Regression guard: the old check was against `/_app/immutable/`
        // AFTER the leading slash was stripped — so every immutable
        // asset fell through to the 1h cache. Make sure we don't
        // reintroduce that off-by-one by looking for the slash-prefixed
        // form.
        assert_eq!(
            cache_policy_for("app/_app/immutable/x.js"),
            "public, max-age=3600",
        );
    }

    #[tokio::test]
    async fn serve_spa_returns_typed_404_for_api_paths() {
        let uri: Uri = "/api/does/not/exist".parse().unwrap();
        let response = serve_spa(uri).await;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            response
                .headers()
                .get(axum::http::header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some("no-store"),
        );

        let bytes = to_bytes(response.into_body(), 1024).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["code"], "not_found");
        assert!(body["message"].as_str().unwrap().contains("No API route"));
    }

    #[tokio::test]
    async fn serve_spa_returns_typed_404_for_bare_api_path() {
        let uri: Uri = "/api".parse().unwrap();
        let response = serve_spa(uri).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn serve_spa_does_not_treat_api_lookalike_as_api() {
        // `/apiv2/x` is NOT an API path — it must not return the typed
        // `no_api_route` envelope, regardless of whether the embedded
        // assets contain a fallback index.
        let uri: Uri = "/apiv2/x".parse().unwrap();
        let response = serve_spa(uri).await;
        let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        // The API-404 branch emits this exact phrase in a JSON envelope;
        // anything else means we went down the SPA / missing-index path.
        assert!(
            !std::str::from_utf8(&bytes)
                .unwrap_or("")
                .contains("No API route matches this path"),
            "apiv2 lookalike was incorrectly treated as API path",
        );
    }
}
