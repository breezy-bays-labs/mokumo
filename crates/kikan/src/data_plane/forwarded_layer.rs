//! Conditional `X-Forwarded-*` handling.
//!
//! In [`DeploymentMode::ReverseProxy`], kikan trusts `X-Forwarded-For` and
//! `X-Forwarded-Proto` from the proxy and stashes the originating IP in
//! request extensions as [`ClientIp`]. In every other mode, both headers are
//! stripped before the request reaches any handler — defense-in-depth against
//! a client that spoofs them to evade rate limiting or audit logging.

use std::net::IpAddr;
use std::task::{Context, Poll};

use http::Request;
use tower::{Layer, Service};

use super::DeploymentMode;

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

impl<S, B> Service<Request<B>> for ForwardedService<S>
where
    S: Service<Request<B>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

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
                if let Some(ip) = first_forwarded_ip(&req) {
                    req.extensions_mut().insert(ClientIp(ip));
                }
            }
        }
        self.inner.call(req)
    }
}

/// Parse the leftmost IP from `X-Forwarded-For`. Per RFC 7239, the first
/// entry is the original client; subsequent entries are intermediate proxies.
fn first_forwarded_ip<B>(req: &Request<B>) -> Option<IpAddr> {
    let raw = req.headers().get("x-forwarded-for")?.to_str().ok()?;
    let first = raw.split(',').next()?.trim();
    // Bracketed IPv6 may appear as `[::1]:port` or `[::1]` in some proxies.
    let candidate = first
        .strip_prefix('[')
        .and_then(|s| s.split(']').next())
        .unwrap_or_else(|| first.split(':').next().unwrap_or(first));
    candidate.parse::<IpAddr>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::Response;
    use std::convert::Infallible;
    use tower::ServiceExt;

    async fn run(mode: DeploymentMode, xff: Option<&'static str>) -> Request<()> {
        // Echo the request through — we inspect the version the inner service
        // receives by stashing it on the response via a closure capture.
        let captured: std::sync::Arc<std::sync::Mutex<Option<Request<()>>>> =
            std::sync::Arc::new(std::sync::Mutex::new(None));
        let cap = captured.clone();
        let inner = tower::service_fn(move |req: Request<()>| {
            let cap = cap.clone();
            async move {
                *cap.lock().unwrap() = Some(clone_request(&req));
                Ok::<_, Infallible>(Response::new(()))
            }
        });
        let svc = ForwardedLayer::for_mode(mode).layer(inner);

        let mut builder = Request::builder().uri("/");
        if let Some(v) = xff {
            builder = builder.header("x-forwarded-for", v);
        }
        let _ = svc.oneshot(builder.body(()).unwrap()).await.unwrap();
        let guard = captured.lock().unwrap();
        clone_request(guard.as_ref().unwrap())
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
        let req = run(DeploymentMode::Lan, Some("203.0.113.7")).await;
        assert!(req.headers().get("x-forwarded-for").is_none());
        assert!(req.extensions().get::<ClientIp>().is_none());
    }

    #[tokio::test]
    async fn internet_mode_strips_xff() {
        let req = run(DeploymentMode::Internet, Some("203.0.113.7")).await;
        assert!(req.headers().get("x-forwarded-for").is_none());
        assert!(req.extensions().get::<ClientIp>().is_none());
    }

    #[tokio::test]
    async fn reverse_proxy_mode_trusts_xff() {
        let req = run(DeploymentMode::ReverseProxy, Some("203.0.113.7")).await;
        let ip = req.extensions().get::<ClientIp>().copied().unwrap();
        assert_eq!(ip.0.to_string(), "203.0.113.7");
    }

    #[tokio::test]
    async fn reverse_proxy_takes_leftmost_entry() {
        let req = run(
            DeploymentMode::ReverseProxy,
            Some("203.0.113.7, 10.0.0.1, 10.0.0.2"),
        )
        .await;
        let ip = req.extensions().get::<ClientIp>().copied().unwrap();
        assert_eq!(ip.0.to_string(), "203.0.113.7");
    }

    #[tokio::test]
    async fn reverse_proxy_handles_bracketed_ipv6() {
        let req = run(DeploymentMode::ReverseProxy, Some("[2001:db8::1]:4443")).await;
        let ip = req.extensions().get::<ClientIp>().copied().unwrap();
        assert_eq!(ip.0.to_string(), "2001:db8::1");
    }

    #[tokio::test]
    async fn reverse_proxy_ignores_malformed_xff() {
        let req = run(DeploymentMode::ReverseProxy, Some("not-an-ip")).await;
        assert!(req.extensions().get::<ClientIp>().is_none());
    }

    #[tokio::test]
    async fn missing_xff_leaves_extension_absent() {
        let req = run(DeploymentMode::ReverseProxy, None).await;
        assert!(req.extensions().get::<ClientIp>().is_none());
    }
}
