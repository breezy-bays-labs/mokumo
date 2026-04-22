//! Per-IP global rate limiter for non-LAN deployments.
//!
//! LAN relies on the per-email / per-user limiters in the control plane for
//! endpoint-specific throttling. Public-facing deployments add this layer to
//! cap total request volume per source IP, independent of endpoint.
//!
//! Uses the sliding-window primitive in [`crate::rate_limit::RateLimiter`].
//! The key is the client IP derived from — in order — [`super::forwarded_layer::ClientIp`]
//! (present only in `ReverseProxy` mode) or the TCP peer address inserted by
//! axum's `ConnectInfo` extractor. If neither is present the request passes
//! through unthrottled: kikan would rather let a request through than
//! reject every request because the rate limiter can't key it.

use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::extract::ConnectInfo;
use http::{Request, Response};
use tower::{Layer, Service};

use super::DeploymentMode;
use super::forwarded_layer::ClientIp;
use crate::rate_limit::RateLimiter;

/// Rate-limit knobs for the per-IP global layer. Non-Lan defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerIpRateLimit {
    pub max_attempts: usize,
    pub window: Duration,
}

impl Default for PerIpRateLimit {
    fn default() -> Self {
        // 600 requests per minute per IP — generous for interactive use,
        // tight enough to stop obvious abuse.
        Self {
            max_attempts: 600,
            window: Duration::from_secs(60),
        }
    }
}

const REJECTION_BODY: &[u8] =
    b"{\"code\":\"RATE_LIMITED\",\"message\":\"Too many requests\",\"details\":null}";

#[derive(Clone)]
pub struct PerIpRateLimiterLayer {
    inner: Option<Arc<RateLimiter>>,
}

impl PerIpRateLimiterLayer {
    /// Construct a layer that throttles per IP when the deployment calls for
    /// it. Returns a pass-through layer in LAN mode.
    pub fn for_mode(mode: DeploymentMode, limit: PerIpRateLimit) -> Self {
        Self {
            inner: if mode.rate_limit_enabled() {
                Some(Arc::new(RateLimiter::new(limit.max_attempts, limit.window)))
            } else {
                None
            },
        }
    }
}

impl<S> Layer<S> for PerIpRateLimiterLayer {
    type Service = PerIpRateLimiterService<S>;
    fn layer(&self, inner: S) -> Self::Service {
        PerIpRateLimiterService {
            inner,
            limiter: self.inner.clone(),
        }
    }
}

#[derive(Clone)]
pub struct PerIpRateLimiterService<S> {
    inner: S,
    limiter: Option<Arc<RateLimiter>>,
}

impl<S, B, ResBody> Service<Request<B>> for PerIpRateLimiterService<S>
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
        let Some(limiter) = self.limiter.clone() else {
            // Pass-through in Lan mode.
            let clone = self.inner.clone();
            let mut inner = std::mem::replace(&mut self.inner, clone);
            return Box::pin(inner.call(req));
        };

        let key = client_ip(&req).map(|ip| ip.to_string());
        if let Some(ref k) = key
            && !limiter.check_and_record(k)
        {
            tracing::warn!(ip = %k, uri = %req.uri(), "per-ip rate limit exceeded");
            return Box::pin(std::future::ready(Ok(build_rejection())));
        }
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);
        Box::pin(inner.call(req))
    }
}

fn client_ip<B>(req: &Request<B>) -> Option<IpAddr> {
    if let Some(ClientIp(ip)) = req.extensions().get::<ClientIp>().copied() {
        return Some(ip);
    }
    req.extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.ip())
}

fn build_rejection<ResBody: From<&'static [u8]>>() -> Response<ResBody> {
    Response::builder()
        .status(429)
        .header("content-type", "application/json")
        .header("cache-control", "no-store")
        .body(ResBody::from(REJECTION_BODY))
        .unwrap()
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

    fn req_with_client(ip: &str) -> Request<()> {
        let mut req = Request::builder().uri("/").body(()).unwrap();
        req.extensions_mut()
            .insert(ClientIp(ip.parse::<IpAddr>().unwrap()));
        req
    }

    #[tokio::test]
    async fn lan_mode_passes_through() {
        let layer = PerIpRateLimiterLayer::for_mode(DeploymentMode::Lan, PerIpRateLimit::default());
        let svc = layer.layer(ok_inner());
        // 10 requests all pass even without a ClientIp extension.
        for _ in 0..10 {
            let resp = svc
                .clone()
                .oneshot(Request::builder().uri("/").body(()).unwrap())
                .await
                .unwrap();
            assert_eq!(resp.status().as_u16(), 200);
        }
    }

    #[tokio::test]
    async fn internet_mode_allows_under_limit_and_rejects_over() {
        let layer = PerIpRateLimiterLayer::for_mode(
            DeploymentMode::Internet,
            PerIpRateLimit {
                max_attempts: 3,
                window: Duration::from_secs(60),
            },
        );
        let svc = layer.layer(ok_inner());
        for _ in 0..3 {
            let resp = svc
                .clone()
                .oneshot(req_with_client("203.0.113.7"))
                .await
                .unwrap();
            assert_eq!(resp.status().as_u16(), 200);
        }
        let resp = svc
            .clone()
            .oneshot(req_with_client("203.0.113.7"))
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 429);
        let body: &[u8] = &resp.into_body();
        let parsed: serde_json::Value = serde_json::from_slice(body).unwrap();
        assert_eq!(parsed["code"], "RATE_LIMITED");
    }

    #[tokio::test]
    async fn internet_mode_isolates_per_ip() {
        let layer = PerIpRateLimiterLayer::for_mode(
            DeploymentMode::Internet,
            PerIpRateLimit {
                max_attempts: 2,
                window: Duration::from_secs(60),
            },
        );
        let svc = layer.layer(ok_inner());
        for _ in 0..2 {
            let resp = svc
                .clone()
                .oneshot(req_with_client("203.0.113.7"))
                .await
                .unwrap();
            assert_eq!(resp.status().as_u16(), 200);
        }
        // Different IP — still under its own limit.
        let resp = svc
            .clone()
            .oneshot(req_with_client("198.51.100.1"))
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 200);
    }

    fn req_with_connect_info(ip: &str) -> Request<()> {
        let mut req = Request::builder().uri("/").body(()).unwrap();
        let sa: SocketAddr = format!("{ip}:54321").parse().unwrap();
        req.extensions_mut().insert(ConnectInfo(sa));
        req
    }

    #[tokio::test]
    async fn internet_mode_keys_on_connect_info_when_client_ip_absent() {
        // ClientIp is only populated in ReverseProxy mode. Internet mode must
        // fall back to axum's ConnectInfo<SocketAddr> — this is what the real
        // production wiring (into_make_service_with_connect_info) inserts.
        let layer = PerIpRateLimiterLayer::for_mode(
            DeploymentMode::Internet,
            PerIpRateLimit {
                max_attempts: 2,
                window: Duration::from_secs(60),
            },
        );
        let svc = layer.layer(ok_inner());
        for _ in 0..2 {
            let resp = svc
                .clone()
                .oneshot(req_with_connect_info("198.51.100.9"))
                .await
                .unwrap();
            assert_eq!(resp.status().as_u16(), 200);
        }
        let resp = svc
            .clone()
            .oneshot(req_with_connect_info("198.51.100.9"))
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 429);
    }

    #[tokio::test]
    async fn missing_client_ip_passes_through_even_when_enabled() {
        // No ClientIp extension, no ConnectInfo — we let the request through.
        let layer = PerIpRateLimiterLayer::for_mode(
            DeploymentMode::Internet,
            PerIpRateLimit {
                max_attempts: 1,
                window: Duration::from_secs(60),
            },
        );
        let svc = layer.layer(ok_inner());
        for _ in 0..5 {
            let resp = svc
                .clone()
                .oneshot(Request::builder().uri("/").body(()).unwrap())
                .await
                .unwrap();
            assert_eq!(resp.status().as_u16(), 200);
        }
    }
}
