//! HTTP security headers middleware.
//!
//! Sets defensive headers on every response: CSP, X-Frame-Options,
//! X-Content-Type-Options, Referrer-Policy, and X-XSS-Protection.
//! Conditionally adds HSTS when the request arrives through Cloudflare Tunnel.

use axum::{
    extract::Request,
    http::{HeaderValue, header},
    middleware::Next,
    response::Response,
};

/// Content-Security-Policy for the SvelteKit SPA.
///
/// `script-src 'unsafe-inline'` is required because SvelteKit adapter-static
/// emits an inline bootstrap `<script>` in index.html. `style-src 'unsafe-inline'`
/// is required because SvelteKit inlines scoped component styles.
/// See ADR `adr-security-headers.md` for rationale and tightening roadmap.
const CSP: &str = "default-src 'self'; script-src 'self' 'unsafe-inline'; \
style-src 'self' 'unsafe-inline'; img-src 'self' data:; \
connect-src 'self'; object-src 'none'; frame-ancestors 'none'";

/// HSTS value: 2 years, include subdomains, no preload (self-hosted domains vary).
const HSTS: &str = "max-age=63072000; includeSubDomains";

/// Middleware that sets security headers on every response.
///
/// HSTS is only set when the request passes through Cloudflare Tunnel,
/// detected by the presence of the `cf-connecting-ip` header.
pub async fn middleware(request: Request, next: Next) -> Response {
    let is_tunnel = request.headers().contains_key("cf-connecting-ip");

    let mut response = next.run(request).await;
    let headers = response.headers_mut();

    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(CSP),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(header::X_XSS_PROTECTION, HeaderValue::from_static("0"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );

    if is_tunnel {
        headers.insert(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static(HSTS),
        );
    }

    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Body, http::StatusCode, routing::get};
    use tower::ServiceExt;

    fn test_app() -> Router {
        Router::new()
            .route("/test", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn(middleware))
    }

    #[tokio::test]
    async fn sets_static_security_headers() {
        let app = test_app();
        let response = app
            .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let h = response.headers();
        assert_eq!(h.get("x-content-type-options").unwrap(), "nosniff");
        assert_eq!(h.get("x-frame-options").unwrap(), "DENY");
        assert_eq!(h.get("x-xss-protection").unwrap(), "0");
        assert_eq!(
            h.get("referrer-policy").unwrap(),
            "strict-origin-when-cross-origin"
        );
        assert!(
            h.get("content-security-policy")
                .unwrap()
                .to_str()
                .unwrap()
                .contains("default-src 'self'")
        );
        assert!(
            h.get("content-security-policy")
                .unwrap()
                .to_str()
                .unwrap()
                .contains("frame-ancestors 'none'")
        );
    }

    #[tokio::test]
    async fn no_hsts_without_tunnel() {
        let app = test_app();
        let response = app
            .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert!(
            response
                .headers()
                .get("strict-transport-security")
                .is_none()
        );
    }

    #[tokio::test]
    async fn sets_hsts_with_cloudflare_tunnel() {
        let app = test_app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/test")
                    .header("cf-connecting-ip", "1.2.3.4")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.headers().get("strict-transport-security").unwrap(),
            "max-age=63072000; includeSubDomains"
        );
    }
}
