use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use http::Request;
use http::header::HOST;
use tower::{Layer, Service};

use crate::data_plane::DataPlaneConfig;

const REJECTION_BODY: &[u8] =
    b"{\"code\":\"HOST_NOT_ALLOWED\",\"message\":\"Host header not allowed\",\"details\":null}";

/// Loopback host patterns that are always accepted alongside whatever the
/// caller supplies. The comparison target emitted by `parse_host` is
/// already lowercased + port-stripped, so these raw strings are safe.
const LOOPBACK_HOSTS: [&str; 3] = ["127.0.0.1", "localhost", "[::1]"];

#[derive(Clone)]
pub struct HostHeaderAllowList {
    allowed: Arc<Vec<String>>,
}

impl HostHeaderAllowList {
    pub fn loopback_only() -> Self {
        Self {
            allowed: Arc::new(LOOPBACK_HOSTS.iter().map(|h| (*h).to_owned()).collect()),
        }
    }

    pub fn new(extra_hosts: Vec<String>) -> Self {
        let mut hosts: Vec<String> = LOOPBACK_HOSTS.iter().map(|h| (*h).to_owned()).collect();
        hosts.extend(extra_hosts.into_iter().map(|h| h.to_ascii_lowercase()));
        Self {
            allowed: Arc::new(hosts),
        }
    }

    /// Build the allowlist from a [`DataPlaneConfig`].
    ///
    /// Always includes the loopback triad (`127.0.0.1`, `localhost`, `[::1]`)
    /// so local health checks and administrative traffic work in every
    /// deployment. Non-loopback entries come from `allowed_hosts`; the
    /// vertical is expected to include mDNS-derived names like
    /// `<shop>.local` when running in [`DeploymentMode::Lan`].
    pub fn from_config(cfg: &DataPlaneConfig) -> Self {
        let mut hosts: Vec<String> = LOOPBACK_HOSTS.iter().map(|h| (*h).to_owned()).collect();
        // `HostPattern::parse` already lowercased and port-stripped the value,
        // so direct string inclusion is correct.
        for pattern in &cfg.allowed_hosts {
            hosts.push(pattern.as_str().to_owned());
        }
        Self {
            allowed: Arc::new(hosts),
        }
    }
}

impl Default for HostHeaderAllowList {
    fn default() -> Self {
        Self::loopback_only()
    }
}

impl<S> Layer<S> for HostHeaderAllowList {
    type Service = HostHeaderAllowListService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        HostHeaderAllowListService {
            inner,
            allowed: Arc::clone(&self.allowed),
        }
    }
}

#[derive(Clone)]
pub struct HostHeaderAllowListService<S> {
    inner: S,
    allowed: Arc<Vec<String>>,
}

impl<S, B, ResBody> Service<Request<B>> for HostHeaderAllowListService<S>
where
    S: Service<Request<B>, Response = http::Response<ResBody>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
    B: Send + 'static,
    ResBody: From<&'static [u8]> + Send + 'static,
{
    type Response = http::Response<ResBody>;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        let mut host_iter = req.headers().get_all(HOST).into_iter();
        let host_value = match (host_iter.next(), host_iter.next()) {
            (Some(hv), None) => hv,
            (None, _) => {
                tracing::warn!(uri = %req.uri(), "host-allowlist: rejected (missing Host header)");
                return Box::pin(std::future::ready(Ok(build_rejection())));
            }
            _ => {
                tracing::warn!(uri = %req.uri(), "host-allowlist: rejected (multiple Host headers)");
                return Box::pin(std::future::ready(Ok(build_rejection())));
            }
        };

        let header_val = match host_value.to_str() {
            Ok(v) => v,
            Err(_) => {
                tracing::warn!(uri = %req.uri(), "host-allowlist: rejected (non-ASCII Host header)");
                return Box::pin(std::future::ready(Ok(build_rejection())));
            }
        };

        let host = parse_host(header_val);

        if !self.allowed.iter().any(|a| a == &host) {
            tracing::warn!(host = %host, uri = %req.uri(), "host-allowlist: rejected (not in allowlist)");
            return Box::pin(std::future::ready(Ok(build_rejection())));
        }

        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);
        Box::pin(inner.call(req))
    }
}

fn parse_host(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    if lower.starts_with('[') {
        match lower.find(']') {
            Some(idx) => lower[..=idx].to_owned(),
            None => lower,
        }
    } else {
        match lower.rsplit_once(':') {
            Some((host, _)) => host.to_owned(),
            None => lower,
        }
    }
}

fn build_rejection<ResBody: From<&'static [u8]>>() -> http::Response<ResBody> {
    http::Response::builder()
        .status(403)
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

    async fn check(host_setup: HostSetup, expected_status: u16) {
        let inner = tower::service_fn(|_req: Request<()>| async {
            Ok::<_, Infallible>(http::Response::new(Vec::<u8>::new()))
        });
        let svc = HostHeaderAllowList::loopback_only().layer(inner);

        let mut req = Request::builder().uri("/test");
        match host_setup {
            HostSetup::None => {}
            HostSetup::Single(h) => {
                req = req.header(HOST, h);
            }
            HostSetup::Multiple(a, b) => {
                req = req.header(HOST, a).header(HOST, b);
            }
        }

        let resp = svc.oneshot(req.body(()).unwrap()).await.unwrap();
        assert_eq!(
            resp.status().as_u16(),
            expected_status,
            "host={host_setup:?}"
        );

        if expected_status == 403 {
            let ct = resp
                .headers()
                .get("content-type")
                .unwrap()
                .to_str()
                .unwrap();
            assert_eq!(ct, "application/json");

            let cc = resp
                .headers()
                .get("cache-control")
                .unwrap()
                .to_str()
                .unwrap();
            assert_eq!(cc, "no-store");

            let body: &[u8] = &resp.into_body();
            let parsed: serde_json::Value = serde_json::from_slice(body).unwrap();
            assert_eq!(parsed["code"], "HOST_NOT_ALLOWED");
            assert_eq!(parsed["message"], "Host header not allowed");
            assert!(parsed["details"].is_null());
        }
    }

    #[derive(Debug)]
    enum HostSetup {
        None,
        Single(&'static str),
        Multiple(&'static str, &'static str),
    }

    #[tokio::test]
    async fn missing_host_rejected() {
        check(HostSetup::None, 403).await;
    }

    #[tokio::test]
    async fn multiple_host_headers_rejected() {
        check(HostSetup::Multiple("127.0.0.1", "evil.com"), 403).await;
    }

    #[tokio::test]
    async fn evil_host_rejected() {
        check(HostSetup::Single("evil.com"), 403).await;
    }

    #[tokio::test]
    async fn loopback_prefix_spoofing_rejected() {
        check(HostSetup::Single("127.0.0.1.evil.com"), 403).await;
    }

    #[tokio::test]
    async fn localhost_prefix_spoofing_rejected() {
        check(HostSetup::Single("localhost.evil.com"), 403).await;
    }

    #[tokio::test]
    async fn percent_encoded_host_rejected() {
        check(HostSetup::Single("%6c%6f%63%61%6c%68%6f%73%74"), 403).await;
    }

    #[tokio::test]
    async fn loopback_no_port_accepted() {
        check(HostSetup::Single("127.0.0.1"), 200).await;
    }

    #[tokio::test]
    async fn loopback_with_port_accepted() {
        check(HostSetup::Single("127.0.0.1:9999"), 200).await;
    }

    #[tokio::test]
    async fn localhost_accepted() {
        check(HostSetup::Single("localhost"), 200).await;
    }

    #[tokio::test]
    async fn localhost_uppercase_accepted() {
        check(HostSetup::Single("LOCALHOST"), 200).await;
    }

    #[tokio::test]
    async fn ipv6_loopback_accepted() {
        check(HostSetup::Single("[::1]"), 200).await;
    }

    #[tokio::test]
    async fn ipv6_loopback_with_port_accepted() {
        check(HostSetup::Single("[::1]:6565"), 200).await;
    }

    #[test]
    fn parse_host_strips_ipv4_port() {
        assert_eq!(parse_host("127.0.0.1:8080"), "127.0.0.1");
    }

    #[test]
    fn parse_host_preserves_ipv4_without_port() {
        assert_eq!(parse_host("127.0.0.1"), "127.0.0.1");
    }

    #[test]
    fn parse_host_strips_ipv6_port() {
        assert_eq!(parse_host("[::1]:6565"), "[::1]");
    }

    #[test]
    fn parse_host_preserves_ipv6_without_port() {
        assert_eq!(parse_host("[::1]"), "[::1]");
    }

    #[test]
    fn parse_host_lowercases() {
        assert_eq!(parse_host("LOCALHOST:8080"), "localhost");
    }

    #[test]
    fn parse_host_no_port_no_brackets() {
        assert_eq!(parse_host("evil.com"), "evil.com");
    }

    #[test]
    fn parse_host_does_not_strip_dot_delimited_suffix() {
        assert_eq!(parse_host("127.0.0.1.evil.com"), "127.0.0.1.evil.com");
    }
}
