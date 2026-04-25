//! Dispatch test for [`CompositeSpaSource`].
//!
//! The composed `/admin/*` shape relies on Axum's `.nest(prefix, router)`
//! stripping the prefix before the inner router runs. SvelteKit's `adapter-static` with
//! `kit.paths.base = "/admin"` emits asset references like
//! `/admin/_app/immutable/chunks/app.js`; the admin SPA's rust-embed
//! bundle contains those files at `_app/immutable/chunks/app.js`
//! (without the prefix). Prefix-strip is the glue.
//!
//! This test uses synthetic [`SpaSource`] fixtures that echo the
//! post-strip URI path in the response body. That proves:
//! - dispatch reached the expected mount or fallback;
//! - the prefix was stripped before the inner router saw the request.
//!
//! Fixture sources are trivially constructible — no rust-embed in kikan
//! (preserves I5).

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, Uri};
use kikan::data_plane::spa::{CompositeSpaSource, SpaSource};
use tower::ServiceExt;

/// A synthetic SPA source that echoes `{label}:{path}` on every request.
struct EchoSpa {
    label: &'static str,
}

impl EchoSpa {
    fn new(label: &'static str) -> Self {
        Self { label }
    }
}

impl SpaSource for EchoSpa {
    fn router(&self) -> Router {
        let label = self.label;
        // Explicit root + catch-all. `Router::fallback` inside a nested
        // router bubbles to the OUTER router's fallback when the stripped
        // path has no matching route, which breaks dispatch for requests
        // like `/admin/` whose stripped form is `/`. Explicit routes keep
        // the request inside the nested router.
        Router::new()
            .route(
                "/",
                axum::routing::any(
                    move |uri: Uri| async move { format!("{label}:{}", uri.path()) },
                ),
            )
            .route(
                "/{*rest}",
                axum::routing::any(
                    move |uri: Uri| async move { format!("{label}:{}", uri.path()) },
                ),
            )
    }
}

/// Build the composed dispatch router the test exercises.
///
/// Platform API + static routes are merged in at the same level as the
/// composite rather than wrapping it — this keeps the composite's
/// internal trailing-slash normalization middleware applied to all
/// requests.
///
/// - `/api/platform/v1/*` → platform API (synthetic string "PLATFORM_API")
/// - `/static/{*rest}` → static handler (synthetic string "STATIC")
/// - `/admin/extensions/{ext_id}/{*path}` → shop SPA (extension subtree)
/// - `/admin/integrations/{int_id}/{*path}` → shop SPA (integration subtree)
/// - `/admin` + subpaths → admin SPA
/// - `/` and everything else → shop SPA (fallback)
fn build_router() -> Router {
    let shop = EchoSpa::new("shop");
    let admin = EchoSpa::new("admin");

    let composite = CompositeSpaSource::new(Box::new(shop))
        .with_mount("/admin/extensions/{ext_id}", Box::new(EchoSpa::new("shop")))
        .with_mount(
            "/admin/integrations/{int_id}",
            Box::new(EchoSpa::new("shop")),
        )
        .with_mount("/admin", Box::new(admin));

    // `NormalizePathLayer::trim_trailing_slash` rewrites the request URI
    // BEFORE route matching, which is necessary because Axum's `.nest`
    // does not match the bare-trailing-slash form `/admin/` (only `/admin`
    // exact and `/admin/<non-empty-tail>`). The layer wraps the whole app
    // so `/admin/` is normalized to `/admin` and nest-matches cleanly.
    use tower::ServiceBuilder;
    use tower_http::normalize_path::NormalizePathLayer;

    let app = Router::new()
        .route(
            "/api/platform/v1/extensions",
            axum::routing::get(|| async { "PLATFORM_API" }),
        )
        .route("/static/{*rest}", axum::routing::get(|| async { "STATIC" }))
        .merge(composite.router());

    Router::new().fallback_service(
        ServiceBuilder::new()
            .layer(NormalizePathLayer::trim_trailing_slash())
            .service(app.into_service::<Body>()),
    )
}

async fn get_body(router: Router, path: &str) -> (StatusCode, String) {
    let resp = router
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

macro_rules! assert_dispatch {
    ($path:expr, $expected_body:expr) => {{
        let router = build_router();
        let (status, body) = get_body(router, $path).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "{} returned {}, expected 200",
            $path,
            status
        );
        assert_eq!(
            body, $expected_body,
            "{} dispatch mismatch: body {:?}",
            $path, body
        );
    }};
}

#[tokio::test]
async fn url1_root_dispatches_to_shop_fallback() {
    assert_dispatch!("/", "shop:/");
}

#[tokio::test]
async fn url2_admin_bare_dispatches_to_admin() {
    // Axum nest normalizes `/admin` → `/admin/`; inner router sees `/`.
    // Either body is acceptable evidence of dispatch to admin.
    let router = build_router();
    let (status, body) = get_body(router, "/admin").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body == "admin:/" || body == "admin:",
        "expected admin dispatch, got {body:?}"
    );
}

#[tokio::test]
async fn url3_admin_slash_dispatches_to_admin() {
    assert_dispatch!("/admin/", "admin:/");
}

#[tokio::test]
async fn url4_admin_asset_path_prefix_stripped() {
    // This is the load-bearing assertion. SvelteKit adapter-static with
    // paths.base='/admin' emits paths like /admin/_app/immutable/chunks/app.js.
    // The admin bundle contains _app/immutable/chunks/app.js (no prefix).
    // After Axum nest strips `/admin`, the inner router sees the correct
    // bundle-relative path.
    assert_dispatch!(
        "/admin/_app/immutable/chunks/app.js",
        "admin:/_app/immutable/chunks/app.js"
    );
}

#[tokio::test]
async fn url5_admin_extensions_list_dispatches_to_admin() {
    assert_dispatch!("/admin/extensions", "admin:/extensions");
}

#[tokio::test]
async fn url6_admin_extensions_subtree_dispatches_to_shop() {
    assert_dispatch!("/admin/extensions/foo", "shop:/");
}

#[tokio::test]
async fn url7_admin_extensions_subtree_deep_dispatches_to_shop() {
    assert_dispatch!("/admin/extensions/foo/bar", "shop:/bar");
}

#[tokio::test]
async fn url8_admin_integrations_subtree_dispatches_to_shop() {
    assert_dispatch!("/admin/integrations/foo", "shop:/");
}

#[tokio::test]
async fn url9_platform_api_dispatches_to_api_router() {
    assert_dispatch!("/api/platform/v1/extensions", "PLATFORM_API");
}

#[tokio::test]
async fn url10_static_asset_dispatches_to_static_handler() {
    assert_dispatch!("/static/logo.png", "STATIC");
}

#[tokio::test]
async fn dispatch_summary_sorts_longest_prefix_first() {
    let composite = CompositeSpaSource::new(Box::new(EchoSpa::new("shop")))
        .with_mount(
            "/admin/integrations/{int_id}",
            Box::new(EchoSpa::new("shop")),
        )
        .with_mount("/admin", Box::new(EchoSpa::new("admin")))
        .with_mount("/admin/extensions/{ext_id}", Box::new(EchoSpa::new("shop")));

    let summary = composite.dispatch_summary();
    assert_eq!(summary.len(), 3);
    assert!(summary[0].len() >= summary[1].len());
    assert!(summary[1].len() >= summary[2].len());
    assert_eq!(summary[summary.len() - 1], "/admin");
}
