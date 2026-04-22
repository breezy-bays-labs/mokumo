//! Composition tests — stack the data-plane middleware layers together for
//! each [`DeploymentMode`] and exercise their observable behavior via
//! `tower::ServiceExt::oneshot`. Per-layer unit tests already pin
//! single-layer behavior; these tests verify the stack composes without
//! cross-layer surprises and that the on-the-wire effects match the plan
//! matrix.

use std::convert::Infallible;

use http::header::{COOKIE, HOST, ORIGIN, SET_COOKIE};
use http::{Method, Request, Response, StatusCode};
use kikan::data_plane::csrf_layer::{CSRF_COOKIE_NAME, CSRF_HEADER_NAME, CsrfLayer};
use kikan::data_plane::forwarded_layer::ForwardedLayer;
use kikan::data_plane::rate_limiter_layer::{PerIpRateLimit, PerIpRateLimiterLayer};
use kikan::middleware::host_allowlist::HostHeaderAllowList;
use kikan::{DataPlaneConfig, DeploymentMode, HostPattern};
use tower::{Service, ServiceBuilder, ServiceExt};

fn ok_inner() -> impl Service<
    Request<()>,
    Response = Response<Vec<u8>>,
    Error = Infallible,
    Future = impl Future<Output = Result<Response<Vec<u8>>, Infallible>> + Send,
> + Clone {
    tower::service_fn(|_req: Request<()>| async {
        Ok::<_, Infallible>(Response::new(Vec::<u8>::new()))
    })
}

fn config_for(mode: DeploymentMode) -> DataPlaneConfig {
    DataPlaneConfig {
        deployment_mode: mode,
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        allowed_origins: vec!["https://shop.example.com".parse().unwrap()],
        allowed_hosts: vec![HostPattern::parse("shop.example.com").unwrap()],
    }
}

/// Stack the same layers [`kikan::Engine::build_router`] applies, minus the
/// session + auth layers (those need a real store and aren't what this test
/// is locking).
fn stack_for(
    mode: DeploymentMode,
) -> impl Service<
    Request<()>,
    Response = Response<Vec<u8>>,
    Error = Infallible,
    Future = impl Future<Output = Result<Response<Vec<u8>>, Infallible>> + Send,
> + Clone {
    let cfg = config_for(mode);
    let host_allowlist = HostHeaderAllowList::from_config(&cfg);
    let forwarded = ForwardedLayer::for_mode(mode);
    let rate_limit = PerIpRateLimiterLayer::for_mode(mode, PerIpRateLimit::default());
    let csrf = CsrfLayer::for_mode(mode, cfg.allowed_origins.clone());

    ServiceBuilder::new()
        .layer(host_allowlist)
        .layer(forwarded)
        .layer(rate_limit)
        .layer(csrf)
        .service(ok_inner())
}

// ---------------------------------------------------------------------------
// Host allowlist
// ---------------------------------------------------------------------------

#[tokio::test]
async fn lan_mode_accepts_loopback_host() {
    let svc = stack_for(DeploymentMode::Lan);
    let req = Request::builder()
        .uri("/")
        .header(HOST, "127.0.0.1")
        .body(())
        .unwrap();
    let resp = svc.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn lan_mode_rejects_unknown_host() {
    let svc = stack_for(DeploymentMode::Lan);
    let req = Request::builder()
        .uri("/")
        .header(HOST, "evil.example.com")
        .body(())
        .unwrap();
    let resp = svc.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn internet_mode_accepts_configured_host() {
    let svc = stack_for(DeploymentMode::Internet);
    // GET does not need CSRF; we just want to confirm the host passes.
    let req = Request::builder()
        .uri("/")
        .header(HOST, "shop.example.com")
        .body(())
        .unwrap();
    let resp = svc.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

// ---------------------------------------------------------------------------
// CSRF gating
// ---------------------------------------------------------------------------

#[tokio::test]
async fn lan_mode_post_without_csrf_passes() {
    let svc = stack_for(DeploymentMode::Lan);
    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/x")
        .header(HOST, "127.0.0.1")
        .body(())
        .unwrap();
    let resp = svc.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn internet_mode_post_without_csrf_is_rejected() {
    let svc = stack_for(DeploymentMode::Internet);
    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/x")
        .header(HOST, "shop.example.com")
        .header(ORIGIN, "https://shop.example.com")
        .body(())
        .unwrap();
    let resp = svc.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn internet_mode_post_with_matching_double_submit_passes() {
    let svc = stack_for(DeploymentMode::Internet);
    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/x")
        .header(HOST, "shop.example.com")
        .header(ORIGIN, "https://shop.example.com")
        .header(COOKIE, format!("{CSRF_COOKIE_NAME}=tok-123"))
        .header(CSRF_HEADER_NAME, "tok-123")
        .body(())
        .unwrap();
    let resp = svc.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn internet_mode_mints_csrf_cookie_on_get() {
    let svc = stack_for(DeploymentMode::Internet);
    let req = Request::builder()
        .uri("/")
        .header(HOST, "shop.example.com")
        .body(())
        .unwrap();
    let resp = svc.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let set_cookie = resp.headers().get(SET_COOKIE).unwrap().to_str().unwrap();
    assert!(set_cookie.contains(CSRF_COOKIE_NAME));
    assert!(set_cookie.contains("Secure"));
    assert!(set_cookie.contains("SameSite=Strict"));
}

#[tokio::test]
async fn reverse_proxy_mode_post_requires_csrf() {
    let svc = stack_for(DeploymentMode::ReverseProxy);
    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/x")
        .header(HOST, "shop.example.com")
        .header(ORIGIN, "https://shop.example.com")
        .body(())
        .unwrap();
    let resp = svc.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

// ---------------------------------------------------------------------------
// Rate limiter
// ---------------------------------------------------------------------------

#[tokio::test]
async fn lan_mode_rate_limiter_is_passthrough() {
    // Tight limit would otherwise kick in — Lan must pass everything.
    let cfg = config_for(DeploymentMode::Lan);
    let svc = ServiceBuilder::new()
        .layer(HostHeaderAllowList::from_config(&cfg))
        .layer(ForwardedLayer::for_mode(DeploymentMode::Lan))
        .layer(PerIpRateLimiterLayer::for_mode(
            DeploymentMode::Lan,
            PerIpRateLimit {
                max_attempts: 1,
                window: std::time::Duration::from_secs(60),
            },
        ))
        .service(ok_inner());
    for _ in 0..10 {
        let req = Request::builder()
            .uri("/")
            .header(HOST, "127.0.0.1")
            .body(())
            .unwrap();
        let resp = svc.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}

// ---------------------------------------------------------------------------
// Forwarded layer
// ---------------------------------------------------------------------------

/// Regression test for the ConnectInfo plumbing fix. The rate limiter keys on
/// [`super::forwarded_layer::ClientIp`] first, then falls back to
/// `ConnectInfo<SocketAddr>` inserted by axum. Before this fix, all four
/// `axum::serve` call sites passed the bare [`axum::Router`] — so
/// `ConnectInfo` was never populated, `client_ip()` returned `None`, and the
/// documented "fail-open" fallback fired on every production request.
/// Binding a real [`TcpListener`] with
/// `into_make_service_with_connect_info::<SocketAddr>()` and sending raw HTTP
/// from the same source is the only way to catch this regression —
/// in-process `tower::oneshot` tests can't exercise the
/// make-service-with-connect-info wiring.
#[tokio::test]
async fn internet_mode_rate_limits_via_real_tcp_with_connect_info() {
    use std::net::SocketAddr;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    use axum::Router;
    use axum::routing::get;

    let mode = DeploymentMode::Internet;
    let cfg = config_for(mode);

    let app: Router = Router::new()
        .route("/", get(|| async { "ok" }))
        .layer(PerIpRateLimiterLayer::for_mode(
            mode,
            PerIpRateLimit {
                max_attempts: 3,
                window: std::time::Duration::from_secs(60),
            },
        ))
        .layer(ForwardedLayer::for_mode(mode))
        .layer(HostHeaderAllowList::from_config(&cfg));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
    });

    async fn http_get_status(addr: SocketAddr) -> u16 {
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let req = b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n";
        stream.write_all(req).await.unwrap();
        let mut buf = Vec::new();
        stream.read_to_end(&mut buf).await.unwrap();
        let text = String::from_utf8_lossy(&buf);
        text.split_whitespace()
            .nth(1)
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(0)
    }

    for _ in 0..3 {
        assert_eq!(http_get_status(addr).await, 200);
    }
    assert_eq!(http_get_status(addr).await, 429);

    server.abort();
}

#[tokio::test]
async fn lan_mode_strips_x_forwarded_for() {
    let svc = stack_for(DeploymentMode::Lan);
    let req = Request::builder()
        .uri("/")
        .header(HOST, "127.0.0.1")
        .header("x-forwarded-for", "203.0.113.7")
        .body(())
        .unwrap();
    let resp = svc.oneshot(req).await.unwrap();
    // Pass-through (no ClientIp was set in the inner service); we can't
    // inspect the inner request directly without more plumbing, but the
    // unit test in forwarded_layer.rs locks the strip semantic. This test
    // confirms the layer participates in the composed stack without
    // breaking anything.
    assert_eq!(resp.status(), StatusCode::OK);
}
