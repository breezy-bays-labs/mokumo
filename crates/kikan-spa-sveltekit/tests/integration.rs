//! Integration tests for the two `SpaSource` impls.
//!
//! Both variants are exercised through a minimal `axum::Router` that
//! mounts the impl as a fallback. Requests cover:
//!
//! - root (`/`) — SPA shell, with no-cache on disk / 1h on embedded (see
//!   the cache-policy comments on each impl);
//! - a mutable asset (`favicon.ico`) — 1h cache;
//! - an immutable asset (`_app/immutable/chunk.js`) — 1y immutable;
//! - a client-side route (`/missing`) — falls back to `index.html`.
//!
//! We assert status + `Cache-Control` + `Content-Type`. We do not assert
//! body bytes beyond length-nonzero — matching `rust-embed`'s or
//! `tower-http`'s byte-level behavior would just re-test those crates.

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use kikan_spa_sveltekit::{SvelteKitSpa, SvelteKitSpaDir};
use rust_embed::RustEmbed;
use tower::ServiceExt;

#[derive(RustEmbed)]
#[folder = "tests/fixtures/fake-spa"]
struct FakeAssets;

fn router_from_embedded() -> Router {
    use kikan::data_plane::spa::SpaSource;
    Router::new().fallback_service(SvelteKitSpa::<FakeAssets>::new().router())
}

fn router_from_disk(dir: std::path::PathBuf) -> Router {
    use kikan::data_plane::spa::SpaSource;
    Router::new().fallback_service(SvelteKitSpaDir::new(dir).router())
}

fn stage_disk_fixture(tmp: &std::path::Path) {
    std::fs::write(
        tmp.join("index.html"),
        "<!doctype html><html><head></head><body>disk spa</body></html>",
    )
    .unwrap();
    std::fs::write(tmp.join("favicon.ico"), b"disk-ico-bytes").unwrap();
    std::fs::create_dir_all(tmp.join("_app/immutable")).unwrap();
    std::fs::write(
        tmp.join("_app/immutable/chunk.js"),
        "export const marker = \"disk-immutable\";",
    )
    .unwrap();
}

async fn call(router: Router, path: &str) -> axum::response::Response {
    router
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

fn header_str(resp: &axum::response::Response, name: header::HeaderName) -> Option<&str> {
    resp.headers().get(name).and_then(|v| v.to_str().ok())
}

// ── Embedded variant ────────────────────────────────────────────────

#[tokio::test]
async fn embedded_serves_index_at_root_with_no_cache() {
    let resp = call(router_from_embedded(), "/").await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        header_str(&resp, header::CACHE_CONTROL),
        Some("no-cache"),
        "SPA shell must not be cached",
    );
    assert_eq!(header_str(&resp, header::CONTENT_TYPE), Some("text/html"),);
}

#[tokio::test]
async fn embedded_serves_favicon_with_one_hour_cache() {
    let resp = call(router_from_embedded(), "/favicon.ico").await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        header_str(&resp, header::CACHE_CONTROL),
        Some("public, max-age=3600"),
    );
}

#[tokio::test]
async fn embedded_serves_immutable_asset_with_one_year_cache() {
    let resp = call(router_from_embedded(), "/_app/immutable/chunk.js").await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        header_str(&resp, header::CACHE_CONTROL),
        Some("public, max-age=31536000, immutable"),
    );
}

#[tokio::test]
async fn embedded_falls_back_to_index_for_unknown_paths() {
    let resp = call(router_from_embedded(), "/missing/deep/link").await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "unknown paths serve the SPA shell so SvelteKit client-side routing works",
    );
    assert_eq!(header_str(&resp, header::CACHE_CONTROL), Some("no-cache"),);
    assert_eq!(header_str(&resp, header::CONTENT_TYPE), Some("text/html"),);
}

// ── Disk variant ────────────────────────────────────────────────────

#[tokio::test]
async fn disk_serves_index_at_root_with_no_cache() {
    let tmp = tempfile::tempdir().unwrap();
    stage_disk_fixture(tmp.path());
    let resp = call(router_from_disk(tmp.path().to_path_buf()), "/").await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(header_str(&resp, header::CACHE_CONTROL), Some("no-cache"),);
    let ct = header_str(&resp, header::CONTENT_TYPE).unwrap();
    assert!(ct.starts_with("text/html"), "got {ct}");
}

#[tokio::test]
async fn disk_serves_favicon_with_one_hour_cache() {
    let tmp = tempfile::tempdir().unwrap();
    stage_disk_fixture(tmp.path());
    let resp = call(router_from_disk(tmp.path().to_path_buf()), "/favicon.ico").await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        header_str(&resp, header::CACHE_CONTROL),
        Some("public, max-age=3600"),
    );
}

#[tokio::test]
async fn disk_serves_immutable_asset_with_one_year_cache() {
    let tmp = tempfile::tempdir().unwrap();
    stage_disk_fixture(tmp.path());
    let resp = call(
        router_from_disk(tmp.path().to_path_buf()),
        "/_app/immutable/chunk.js",
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        header_str(&resp, header::CACHE_CONTROL),
        Some("public, max-age=31536000, immutable"),
    );
}

#[tokio::test]
async fn disk_falls_back_to_index_for_unknown_paths() {
    let tmp = tempfile::tempdir().unwrap();
    stage_disk_fixture(tmp.path());
    let resp = call(router_from_disk(tmp.path().to_path_buf()), "/missing/deep").await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(header_str(&resp, header::CACHE_CONTROL), Some("no-cache"),);
    let ct = header_str(&resp, header::CONTENT_TYPE).unwrap();
    assert!(ct.starts_with("text/html"), "got {ct}");
}
