//! Minimal CSRF protection for non-LAN deployments.
//!
//! Two overlapping checks applied to state-changing methods
//! (POST/PUT/PATCH/DELETE):
//!
//! 1. **Origin-header allowlist.** The browser always sets `Origin` on
//!    cross-origin requests and on same-origin non-GET requests in modern
//!    browsers. If `Origin` is present we require it to be in
//!    [`super::DataPlaneConfig::allowed_origins`]; otherwise the request is
//!    rejected. A missing `Origin` falls back to `Referer`'s scheme+authority.
//! 2. **Double-submit cookie.** A CSRF cookie is minted on every request that
//!    does not already carry one; state-changing requests must echo its
//!    value in the `X-CSRF-Token` header. The cookie is `Secure`, `HttpOnly=false`
//!    (JS reads it to attach the header), `SameSite=Strict`.
//!
//! A request fails CSRF if *either* check fails. Lan mode bypasses this layer
//! entirely (see [`CsrfLayer::for_mode`]).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use http::header::{CACHE_CONTROL, CONTENT_TYPE, COOKIE, ORIGIN, REFERER, SET_COOKIE};
use http::{HeaderValue, Method, Request, Response, StatusCode};
use tower::{Layer, Service};

use super::DeploymentMode;

/// Cookie name for the CSRF double-submit token.
pub const CSRF_COOKIE_NAME: &str = "csrf_token";
/// Request header carrying the echoed token.
pub const CSRF_HEADER_NAME: &str = "x-csrf-token";

const REJECTION_BODY: &[u8] =
    b"{\"code\":\"CSRF_REJECTED\",\"message\":\"CSRF check failed\",\"details\":null}";

#[derive(Clone)]
pub struct CsrfLayer {
    enabled: bool,
    allowed_origins: Arc<Vec<HeaderValue>>,
    cookie_secure: bool,
}

impl CsrfLayer {
    /// Build a CSRF layer. Returns a pass-through layer in Lan mode.
    pub fn for_mode(mode: DeploymentMode, allowed_origins: Vec<HeaderValue>) -> Self {
        Self {
            enabled: mode.csrf_enabled(),
            allowed_origins: Arc::new(allowed_origins),
            cookie_secure: mode.cookies_require_secure(),
        }
    }
}

impl<S> Layer<S> for CsrfLayer {
    type Service = CsrfService<S>;
    fn layer(&self, inner: S) -> Self::Service {
        CsrfService {
            inner,
            enabled: self.enabled,
            allowed_origins: self.allowed_origins.clone(),
            cookie_secure: self.cookie_secure,
        }
    }
}

#[derive(Clone)]
pub struct CsrfService<S> {
    inner: S,
    enabled: bool,
    allowed_origins: Arc<Vec<HeaderValue>>,
    cookie_secure: bool,
}

impl<S, B, ResBody> Service<Request<B>> for CsrfService<S>
where
    S: Service<Request<B>, Response = Response<ResBody>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
    B: Send + 'static,
    ResBody: From<&'static [u8]> + Send + 'static,
{
    type Response = Response<ResBody>;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);

        if !self.enabled {
            return Box::pin(inner.call(req));
        }

        let existing_cookie = extract_csrf_cookie(&req);
        let method = req.method().clone();
        let unsafe_method = matches!(
            method,
            Method::POST | Method::PUT | Method::PATCH | Method::DELETE
        );

        if unsafe_method {
            // 1. Origin / Referer allowlist.
            let origin_ok = check_origin(&req, &self.allowed_origins);
            if !origin_ok {
                tracing::warn!(
                    method = %method,
                    uri = %req.uri(),
                    "csrf: rejected — Origin/Referer not in allowlist"
                );
                return Box::pin(std::future::ready(Ok(build_rejection())));
            }

            // 2. Double-submit cookie vs header.
            let header_token = req
                .headers()
                .get(CSRF_HEADER_NAME)
                .and_then(|h| h.to_str().ok())
                .map(|s| s.to_owned());
            let cookie_token = existing_cookie.clone();
            let tokens_match = match (&cookie_token, &header_token) {
                (Some(c), Some(h)) => constant_time_eq(c.as_bytes(), h.as_bytes()),
                _ => false,
            };
            if !tokens_match {
                tracing::warn!(
                    method = %method,
                    uri = %req.uri(),
                    has_cookie = cookie_token.is_some(),
                    has_header = header_token.is_some(),
                    "csrf: rejected — double-submit token missing or mismatched"
                );
                return Box::pin(std::future::ready(Ok(build_rejection())));
            }
        }

        // Mint a fresh cookie if the request did not carry one. Even
        // unsafe-method requests reach this branch after passing both
        // checks above (we don't rotate on every request — cookie stays
        // stable until the browser drops it).
        let mint_cookie = existing_cookie.is_none();
        let cookie_secure = self.cookie_secure;
        let fut = inner.call(req);
        Box::pin(async move {
            let mut resp = fut.await?;
            if mint_cookie {
                let token = new_token();
                let cookie = format_cookie(&token, cookie_secure);
                if let Ok(hv) = HeaderValue::from_str(&cookie) {
                    resp.headers_mut().append(SET_COOKIE, hv);
                }
            }
            Ok(resp)
        })
    }
}

fn extract_csrf_cookie<B>(req: &Request<B>) -> Option<String> {
    for cookies in req.headers().get_all(COOKIE) {
        // Skip headers that aren't valid UTF-8 — a single junk header must not
        // abort the scan, or a stray non-ASCII Cookie header would make the
        // CSRF cookie appear missing, forcing a mint on every request and
        // breaking subsequent POSTs.
        let Ok(raw) = cookies.to_str() else {
            tracing::debug!("csrf: skipping non-ASCII Cookie header");
            continue;
        };
        for pair in raw.split(';') {
            let pair = pair.trim();
            if let Some(value) = pair.strip_prefix(&format!("{CSRF_COOKIE_NAME}=")) {
                return Some(value.to_owned());
            }
        }
    }
    None
}

fn check_origin<B>(req: &Request<B>, allowed: &[HeaderValue]) -> bool {
    // Allowlist entries are lowercased at CLI parse time; the comparison is
    // byte-exact so the header must be lowercased too (browsers emit lowercase
    // scheme + authority, but we don't want to depend on that).
    if let Some(origin) = req.headers().get(ORIGIN).and_then(|h| h.to_str().ok()) {
        let origin_lc = origin.to_ascii_lowercase();
        return allowed.iter().any(|a| a.as_bytes() == origin_lc.as_bytes());
    }
    if let Some(referer) = req.headers().get(REFERER).and_then(|h| h.to_str().ok())
        && let Some(origin_from_referer) = referer_origin(referer)
    {
        return allowed
            .iter()
            .any(|a| a.as_bytes() == origin_from_referer.as_bytes());
    }
    // No Origin and no usable Referer — browsers always set at least one on
    // cross-origin POSTs, so treat the absence as suspect.
    false
}

fn referer_origin(referer: &str) -> Option<String> {
    let (scheme, rest) = referer.split_once("://")?;
    let authority = rest.split('/').next()?;
    if scheme.is_empty() || authority.is_empty() {
        return None;
    }
    Some(format!("{scheme}://{authority}").to_ascii_lowercase())
}

fn new_token() -> String {
    use rand::RngExt;
    let mut rng = rand::rng();
    let mut bytes = [0u8; 32];
    for b in bytes.iter_mut() {
        *b = rng.random();
    }
    // Lowercase hex, 64 chars. Cheap, no extra deps, cookie-safe.
    let mut out = String::with_capacity(bytes.len() * 2);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn format_cookie(token: &str, secure: bool) -> String {
    // HttpOnly is deliberately omitted: JS needs to read this cookie to
    // echo it into the X-CSRF-Token header. SameSite=Strict blocks
    // cross-site navigations; Path=/ covers the full app.
    let mut out = format!("{CSRF_COOKIE_NAME}={token}; Path=/; SameSite=Strict");
    if secure {
        out.push_str("; Secure");
    }
    out
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn build_rejection<ResBody: From<&'static [u8]>>() -> Response<ResBody> {
    let mut response = Response::new(ResBody::from(REJECTION_BODY));
    *response.status_mut() = StatusCode::FORBIDDEN;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::Infallible;
    use tower::ServiceExt;

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

    fn layer_internet() -> CsrfLayer {
        CsrfLayer::for_mode(
            DeploymentMode::Internet,
            vec![HeaderValue::from_static("https://shop.example.com")],
        )
    }

    fn get_request() -> Request<()> {
        Request::builder()
            .method(Method::GET)
            .uri("/api/x")
            .body(())
            .unwrap()
    }

    fn post_request(
        origin: Option<&'static str>,
        cookie: Option<&str>,
        header: Option<&str>,
    ) -> Request<()> {
        let mut b = Request::builder().method(Method::POST).uri("/api/x");
        if let Some(o) = origin {
            b = b.header(ORIGIN, o);
        }
        if let Some(c) = cookie {
            b = b.header(COOKIE, format!("{CSRF_COOKIE_NAME}={c}"));
        }
        if let Some(h) = header {
            b = b.header(CSRF_HEADER_NAME, h);
        }
        b.body(()).unwrap()
    }

    #[tokio::test]
    async fn lan_mode_is_pass_through() {
        let layer = CsrfLayer::for_mode(DeploymentMode::Lan, vec![]);
        let svc = layer.layer(ok_inner());
        let resp = svc.oneshot(post_request(None, None, None)).await.unwrap();
        assert_eq!(resp.status().as_u16(), 200);
    }

    #[tokio::test]
    async fn get_in_internet_mode_mints_cookie() {
        let svc = layer_internet().layer(ok_inner());
        let resp = svc.oneshot(get_request()).await.unwrap();
        assert_eq!(resp.status().as_u16(), 200);
        let set_cookie = resp.headers().get(SET_COOKIE).unwrap().to_str().unwrap();
        assert!(set_cookie.contains(CSRF_COOKIE_NAME));
        assert!(set_cookie.contains("Secure"));
        assert!(set_cookie.contains("SameSite=Strict"));
    }

    #[tokio::test]
    async fn post_without_origin_is_rejected() {
        let svc = layer_internet().layer(ok_inner());
        let resp = svc
            .oneshot(post_request(None, Some("tok"), Some("tok")))
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 403);
    }

    #[tokio::test]
    async fn post_with_bad_origin_is_rejected() {
        let svc = layer_internet().layer(ok_inner());
        let resp = svc
            .oneshot(post_request(
                Some("https://evil.example.com"),
                Some("tok"),
                Some("tok"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 403);
    }

    #[tokio::test]
    async fn post_with_missing_header_is_rejected() {
        let svc = layer_internet().layer(ok_inner());
        let resp = svc
            .oneshot(post_request(
                Some("https://shop.example.com"),
                Some("tok"),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 403);
    }

    #[tokio::test]
    async fn post_with_mismatched_token_is_rejected() {
        let svc = layer_internet().layer(ok_inner());
        let resp = svc
            .oneshot(post_request(
                Some("https://shop.example.com"),
                Some("tok-a"),
                Some("tok-b"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 403);
        let body: &[u8] = &resp.into_body();
        let parsed: serde_json::Value = serde_json::from_slice(body).unwrap();
        assert_eq!(parsed["code"], "CSRF_REJECTED");
    }

    #[tokio::test]
    async fn post_with_matching_double_submit_passes() {
        let svc = layer_internet().layer(ok_inner());
        let resp = svc
            .oneshot(post_request(
                Some("https://shop.example.com"),
                Some("tok-abc"),
                Some("tok-abc"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 200);
    }

    #[tokio::test]
    async fn post_with_referer_fallback_passes() {
        // No Origin, but Referer derives an acceptable origin.
        let mut req = Request::builder()
            .method(Method::POST)
            .uri("/api/x")
            .header(REFERER, "https://shop.example.com/page")
            .header(COOKIE, format!("{CSRF_COOKIE_NAME}=tok"))
            .header(CSRF_HEADER_NAME, "tok")
            .body(())
            .unwrap();
        // Nuke Origin even though we didn't set it, just to be explicit.
        req.headers_mut().remove(ORIGIN);
        let svc = layer_internet().layer(ok_inner());
        let resp = svc.oneshot(req).await.unwrap();
        assert_eq!(resp.status().as_u16(), 200);
    }

    #[test]
    fn referer_origin_parses_scheme_and_authority() {
        assert_eq!(
            referer_origin("https://shop.example.com/page"),
            Some("https://shop.example.com".to_owned())
        );
        assert_eq!(referer_origin("not a url"), None);
    }

    #[test]
    fn referer_origin_lowercases() {
        assert_eq!(
            referer_origin("HTTPS://Shop.EXAMPLE.com/path"),
            Some("https://shop.example.com".to_owned())
        );
    }

    #[tokio::test]
    async fn post_with_uppercase_origin_header_still_matches_lowercase_allowlist() {
        let svc = layer_internet().layer(ok_inner());
        let resp = svc
            .oneshot(post_request(
                Some("HTTPS://Shop.Example.COM"),
                Some("tok"),
                Some("tok"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 200);
    }

    #[test]
    fn constant_time_eq_matches_when_equal() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
    }

    #[test]
    fn format_cookie_includes_secure_when_requested() {
        let c = format_cookie("tok", true);
        assert!(c.contains("Secure"));
        let c = format_cookie("tok", false);
        assert!(!c.contains("Secure"));
    }
}
