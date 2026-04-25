//! End-to-end proof that the cookie-path assertion middleware fires when
//! a downstream handler emits a `Set-Cookie` missing the root `Path`.
//!
//! Five tests, paired by direction:
//!
//! - `session_cookie_with_path_root_passes_through` — well-formed session
//!   cookie (`Path=/`) is preserved and the response body + headers arrive
//!   unchanged. Exercises the middleware's no-op path.
//! - `non_session_cookie_is_ignored` — a `csrf=token; Path=/admin` cookie
//!   passes through untouched; the middleware only watches the session
//!   cookie name.
//! - `response_without_set_cookie_is_untouched` — responses with no
//!   `Set-Cookie` header at all bypass the middleware entirely.
//! - `session_cookie_without_path_root_panics_in_debug` — a session cookie
//!   scoped to `/admin` triggers the middleware's `debug_assert!` panic.
//!   Debug builds only; the test is gated with `#[cfg(debug_assertions)]`
//!   and uses the `#[should_panic(expected = ...)]` attribute to capture
//!   the failure.
//! - `session_cookie_without_path_root_warns_in_release` — the same
//!   misconfigured cookie produces a `tracing::warn!` and a successful
//!   response (the release-mode "noisier-but-degrading" branch). Release
//!   builds only; gated with `#[cfg(not(debug_assertions))]`.
//!
//! Covers `adr-tauri-http-not-ipc` Commitment 7 in the test suite so
//! regressions in `tower-sessions`, the cookie builder, or a future
//! session-name rename surface at CI time instead of production.

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::middleware::from_fn;
use axum::response::Response;
use kikan::data_plane::cookie_path_layer::assert_session_cookie_path_root;
use tower::ServiceExt;

async fn ok_with_cookie(cookie: &'static str) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::SET_COOKIE, cookie)
        .body(Body::from("body"))
        .unwrap()
}

fn app_emitting(cookie: &'static str) -> Router {
    Router::new()
        .route(
            "/login",
            axum::routing::post(move || ok_with_cookie(cookie)),
        )
        .layer(from_fn(assert_session_cookie_path_root))
}

#[tokio::test]
async fn session_cookie_with_path_root_passes_through() {
    let router = app_emitting("id=abc; Path=/; HttpOnly; SameSite=Lax");
    let resp = router
        .oneshot(
            Request::builder()
                .uri("/login")
                .method("POST")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let set_cookie = resp
        .headers()
        .get(header::SET_COOKIE)
        .expect("cookie preserved")
        .to_str()
        .unwrap();
    assert!(set_cookie.contains("id=abc"));
    assert!(set_cookie.contains("Path=/"));
}

#[tokio::test]
async fn non_session_cookie_is_ignored() {
    // A CSRF cookie scoped to `/admin` must not trigger the session
    // invariant — the middleware only cares about the session cookie name.
    let router = app_emitting("csrf=token; Path=/admin; HttpOnly");
    let resp = router
        .oneshot(
            Request::builder()
                .uri("/login")
                .method("POST")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[cfg(debug_assertions)]
#[tokio::test]
#[should_panic(expected = "session cookie must carry Path=/")]
async fn session_cookie_without_path_root_panics_in_debug() {
    // The middleware panics in debug builds when a session cookie lacks
    // `Path=/`. The release-mode counterpart below covers warn-and-continue.
    let router = app_emitting("id=abc; Path=/admin; HttpOnly");
    let _ = router
        .oneshot(
            Request::builder()
                .uri("/login")
                .method("POST")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
}

#[cfg(not(debug_assertions))]
#[tokio::test]
async fn session_cookie_without_path_root_warns_in_release() {
    // Release builds choose "noisier-but-degrading" over "fail closed" —
    // the middleware emits `tracing::warn!` and forwards the response
    // unchanged so a library regression doesn't lock users out of a live
    // install. Pinning the non-panic path here so a future change that
    // promotes release behavior to a panic surfaces under `cargo test
    // --release`.
    let router = app_emitting("id=abc; Path=/admin; HttpOnly");
    let resp = router
        .oneshot(
            Request::builder()
                .uri("/login")
                .method("POST")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn response_without_set_cookie_is_untouched() {
    let router = Router::new()
        .route("/ping", axum::routing::get(|| async { "pong" }))
        .layer(from_fn(assert_session_cookie_path_root));
    let resp = router
        .oneshot(Request::builder().uri("/ping").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp.headers().get(header::SET_COOKIE).is_none());
}
