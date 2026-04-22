//! Conditional `X-Forwarded-*` handling.
//!
//! In [`DeploymentMode::ReverseProxy`], kikan trusts `X-Forwarded-For` and
//! `X-Forwarded-Proto` from the proxy and stashes the originating IP in
//! request extensions as [`ClientIp`]. In every other mode, both headers are
//! stripped before the request reaches any handler — defense-in-depth against
//! a client that spoofs them to evade rate limiting or audit logging.

use std::future::Future;
use std::net::IpAddr;
use std::pin::Pin;
use std::task::{Context, Poll};

use http::header::{CACHE_CONTROL, CONTENT_TYPE};
use http::{HeaderValue, Request, Response, StatusCode};
use tower::{Layer, Service};

use super::DeploymentMode;

const MALFORMED_XFF_BODY: &[u8] =
    b"{\"code\":\"MALFORMED_FORWARDED\",\"message\":\"malformed X-Forwarded-For\",\"details\":null}";

/// Originating client IP, populated by [`ForwardedLayer`] when the deployment
/// trusts `X-Forwarded-For`. Absent otherwise (downstream layers should fall
/// back to the socket peer address).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientIp(pub IpAddr);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Trust,
    Strip,
}

#[derive(Clone)]
pub struct ForwardedLayer {
    mode: Mode,
}

impl ForwardedLayer {
    pub fn for_mode(mode: DeploymentMode) -> Self {
        Self {
            mode: if mode.trust_forwarded() {
                Mode::Trust
            } else {
                Mode::Strip
            },
        }
    }
}

impl<S> Layer<S> for ForwardedLayer {
    type Service = ForwardedService<S>;
    fn layer(&self, inner: S) -> Self::Service {
        ForwardedService {
            inner,
            mode: self.mode,
        }
    }
}

#[derive(Clone)]
pub struct ForwardedService<S> {
    inner: S,
    mode: Mode,
}

impl<S, B, ResBody> Service<Request<B>> for ForwardedService<S>
where
    S: Service<Request<B>, Response = Response<ResBody>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
    B: Send + 'static,
    ResBody: From<&'static [u8]> + Send + 'static,
{
    type Response = Response<ResBody>;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Response<ResBody>, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<B>) -> Self::Future {
        match self.mode {
            Mode::Strip => {
                req.headers_mut().remove("x-forwarded-for");
                req.headers_mut().remove("x-forwarded-proto");
                req.headers_mut().remove("x-forwarded-host");
            }
            Mode::Trust => {
                match parse_forwarded_ip(&req) {
                    ForwardedIp::Present(ip) => {
                        req.extensions_mut().insert(ClientIp(ip));
                    }
                    ForwardedIp::Absent => {
                        // XFF absent in ReverseProxy mode is odd (a
                        // well-behaved proxy always supplies it) but not
                        // fatal; downstream layers can fall back to
                        // ConnectInfo (= the proxy's peer address).
                    }
                    ForwardedIp::Malformed => {
                        tracing::error!(
                            uri = %req.uri(),
                            "forwarded-layer: rejecting request — `X-Forwarded-For` \
                             header present but unparseable as client IP"
                        );
                        return Box::pin(std::future::ready(Ok(malformed_rejection())));
                    }
                }
            }
        }
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);
        Box::pin(inner.call(req))
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ForwardedIp {
    Present(IpAddr),
    Absent,
    Malformed,
}

/// Parse the leftmost IP from `X-Forwarded-For`. Per RFC 7239, the first
/// entry is the original client; subsequent entries are intermediate proxies.
///
/// Tries three parse forms, in order: bare address (handles `127.0.0.1`
/// and bare IPv6 like `2001:db8::1` that nginx has been observed emitting
/// unbracketed), bracketed IPv6 `[::1]` / `[::1]:4443`, and finally
/// `ipv4:port`.
fn parse_forwarded_ip<B>(req: &Request<B>) -> ForwardedIp {
    let Some(raw) = req.headers().get("x-forwarded-for") else {
        return ForwardedIp::Absent;
    };
    let Ok(raw) = raw.to_str() else {
        return ForwardedIp::Malformed;
    };
    let Some(first) = raw.split(',').next() else {
        return ForwardedIp::Malformed;
    };
    let first = first.trim();
    if first.is_empty() {
        return ForwardedIp::Malformed;
    }
    if let Ok(ip) = first.parse::<IpAddr>() {
        return ForwardedIp::Present(ip);
    }
    if let Some(rest) = first.strip_prefix('[')
        && let Some((host, _after)) = rest.split_once(']')
        && let Ok(ip) = host.parse::<IpAddr>()
    {
        return ForwardedIp::Present(ip);
    }
    if let Some((host, _port)) = first.rsplit_once(':')
        && let Ok(ip) = host.parse::<IpAddr>()
    {
        return ForwardedIp::Present(ip);
    }
    ForwardedIp::Malformed
}

fn malformed_rejection<ResBody: From<&'static [u8]>>() -> Response<ResBody> {
    let mut response = Response::new(ResBody::from(MALFORMED_XFF_BODY));
    *response.status_mut() = StatusCode::BAD_REQUEST;
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
    use http::Response;
    use std::convert::Infallible;
    use tower::{Service, ServiceExt};

    enum Outcome {
        /// Inner handler was reached and observed this request.
        Reached(Request<()>),
        /// The layer short-circuited with this response (e.g. 400 on
        /// malformed XFF).
        ShortCircuit(Response<Vec<u8>>),
    }

    async fn run(mode: DeploymentMode, xff: Option<&'static str>) -> Outcome {
        let captured: std::sync::Arc<std::sync::Mutex<Option<Request<()>>>> =
            std::sync::Arc::new(std::sync::Mutex::new(None));
        let cap = captured.clone();
        let inner = tower::service_fn(move |req: Request<()>| {
            let cap = cap.clone();
            async move {
                *cap.lock().unwrap() = Some(clone_request(&req));
                Ok::<_, Infallible>(Response::new(Vec::<u8>::new()))
            }
        });
        let mut svc = ForwardedLayer::for_mode(mode).layer(inner);

        let mut builder = Request::builder().uri("/");
        if let Some(v) = xff {
            builder = builder.header("x-forwarded-for", v);
        }
        let resp = svc
            .ready()
            .await
            .unwrap()
            .call(builder.body(()).unwrap())
            .await
            .unwrap();
        let guard = captured.lock().unwrap();
        match guard.as_ref() {
            Some(req) => Outcome::Reached(clone_request(req)),
            None => Outcome::ShortCircuit(resp),
        }
    }

    async fn run_reached(mode: DeploymentMode, xff: Option<&'static str>) -> Request<()> {
        match run(mode, xff).await {
            Outcome::Reached(req) => req,
            Outcome::ShortCircuit(resp) => {
                panic!(
                    "expected request to reach inner service, got short-circuit status {}",
                    resp.status()
                )
            }
        }
    }

    fn clone_request(req: &Request<()>) -> Request<()> {
        let mut b = Request::builder().method(req.method()).uri(req.uri());
        for (k, v) in req.headers() {
            b = b.header(k, v);
        }
        let mut out = b.body(()).unwrap();
        if let Some(ip) = req.extensions().get::<ClientIp>() {
            out.extensions_mut().insert(*ip);
        }
        out
    }

    #[tokio::test]
    async fn lan_mode_strips_xff() {
        let req = run_reached(DeploymentMode::Lan, Some("203.0.113.7")).await;
        assert!(req.headers().get("x-forwarded-for").is_none());
        assert!(req.extensions().get::<ClientIp>().is_none());
    }

    #[tokio::test]
    async fn internet_mode_strips_xff() {
        let req = run_reached(DeploymentMode::Internet, Some("203.0.113.7")).await;
        assert!(req.headers().get("x-forwarded-for").is_none());
        assert!(req.extensions().get::<ClientIp>().is_none());
    }

    #[tokio::test]
    async fn reverse_proxy_mode_trusts_xff() {
        let req = run_reached(DeploymentMode::ReverseProxy, Some("203.0.113.7")).await;
        let ip = req.extensions().get::<ClientIp>().copied().unwrap();
        assert_eq!(ip.0.to_string(), "203.0.113.7");
    }

    #[tokio::test]
    async fn reverse_proxy_takes_leftmost_entry() {
        let req = run_reached(
            DeploymentMode::ReverseProxy,
            Some("203.0.113.7, 10.0.0.1, 10.0.0.2"),
        )
        .await;
        let ip = req.extensions().get::<ClientIp>().copied().unwrap();
        assert_eq!(ip.0.to_string(), "203.0.113.7");
    }

    #[tokio::test]
    async fn reverse_proxy_handles_bracketed_ipv6() {
        let req = run_reached(DeploymentMode::ReverseProxy, Some("[2001:db8::1]:4443")).await;
        let ip = req.extensions().get::<ClientIp>().copied().unwrap();
        assert_eq!(ip.0.to_string(), "2001:db8::1");
    }

    #[tokio::test]
    async fn reverse_proxy_handles_bare_ipv6() {
        // Some proxies (nginx has been observed) emit unbracketed IPv6.
        let req = run_reached(DeploymentMode::ReverseProxy, Some("2001:db8::1")).await;
        let ip = req.extensions().get::<ClientIp>().copied().unwrap();
        assert_eq!(ip.0.to_string(), "2001:db8::1");
    }

    #[tokio::test]
    async fn reverse_proxy_rejects_malformed_xff() {
        // Previously: silent pass-through, ClientIp extension absent, rate
        // limiter collapses to the proxy's peer address — every attacker
        // behind the proxy shares one bucket. Now: fail-closed with 400.
        match run(DeploymentMode::ReverseProxy, Some("not-an-ip")).await {
            Outcome::ShortCircuit(resp) => {
                assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
                let body: &[u8] = resp.body();
                let parsed: serde_json::Value = serde_json::from_slice(body).unwrap();
                assert_eq!(parsed["code"], "MALFORMED_FORWARDED");
            }
            Outcome::Reached(_) => panic!("expected 400 short-circuit on malformed XFF"),
        }
    }

    #[tokio::test]
    async fn reverse_proxy_rejects_empty_xff() {
        // An empty-string XFF is no better than a malformed one — reject.
        match run(DeploymentMode::ReverseProxy, Some("   ")).await {
            Outcome::ShortCircuit(resp) => {
                assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
            }
            Outcome::Reached(_) => panic!("expected 400 short-circuit on empty XFF"),
        }
    }

    #[tokio::test]
    async fn missing_xff_leaves_extension_absent() {
        // No XFF header at all is tolerated — downstream falls back to
        // ConnectInfo.
        let req = run_reached(DeploymentMode::ReverseProxy, None).await;
        assert!(req.extensions().get::<ClientIp>().is_none());
    }

    #[tokio::test]
    async fn strip_modes_do_not_care_about_malformed_xff() {
        // In Strip modes the header is thrown away, so a malformed value is a
        // non-event — Strip must not 400.
        let req = run_reached(DeploymentMode::Internet, Some("not-an-ip")).await;
        assert!(req.headers().get("x-forwarded-for").is_none());
        let req = run_reached(DeploymentMode::Lan, Some("not-an-ip")).await;
        assert!(req.headers().get("x-forwarded-for").is_none());
    }
}
