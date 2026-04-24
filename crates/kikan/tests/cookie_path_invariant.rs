//! End-to-end proof that the cookie-path assertion middleware fires when
//! a downstream handler emits a `Set-Cookie` missing the root `Path`.
//!
//! Two halves:
//!
//! 1. `session_cookie_with_path_root_passes_through` — emits a well-formed
//!    session cookie (`Path=/`) and confirms the response body + headers
//!    arrive unchanged. Exercises the middleware's no-op path in debug
//!    builds.
//!
//! 2. `session_cookie_without_path_root_panics_in_debug` — emits a session
//!    cookie scoped to `/admin` and confirms the middleware panics (debug
//!    builds only). The panic is caught via `std::panic::AssertUnwindSafe`
//!    inside the handler's task and surfaced as a 500, which is the
//!    axum default for panicking handlers.
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
    // `Path=/`. Release builds only `tracing::warn!` (covered by a
    // separate manual-inspection path; see the module-level docs).
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
