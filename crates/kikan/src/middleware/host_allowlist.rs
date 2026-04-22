use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use http::header::{CACHE_CONTROL, CONTENT_TYPE, HOST};
use http::{HeaderValue, Request, Response, StatusCode};
use tower::{Layer, Service};

use crate::data_plane::{DataPlaneConfig, DeploymentMode};

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
    /// In [`DeploymentMode::Lan`] the loopback triad (`127.0.0.1`, `localhost`,
    /// `[::1]`) is admitted automatically so local health checks and admin
    /// probes work without any config. In `Internet` / `ReverseProxy` **only**
    /// the explicit `allowed_hosts` are admitted — operators who want a
    /// loopback health probe add `--allowed-host 127.0.0.1` themselves. This
    /// avoids a latent authz bypass if a future handler ever decides that
    /// `Host: 127.0.0.1` implies a privileged local caller.
    ///
    /// `HostPattern::parse` already lowercased and port-stripped the values,
    /// so direct string inclusion is a correct exact-match comparison.
    pub fn from_config(cfg: &DataPlaneConfig) -> Self {
        let mut hosts: Vec<String> = if cfg.deployment_mode == DeploymentMode::Lan {
            LOOPBACK_HOSTS.iter().map(|h| (*h).to_owned()).collect()
        } else {
            Vec::with_capacity(cfg.allowed_hosts.len())
        };
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

    mod from_config {
        use super::super::*;
        use crate::data_plane::{DataPlaneConfig, DeploymentMode, HostPattern};
        use std::convert::Infallible;
        use tower::ServiceExt;

        fn cfg(mode: DeploymentMode, hosts: Vec<&str>) -> DataPlaneConfig {
            DataPlaneConfig {
                deployment_mode: mode,
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                allowed_origins: Vec::new(),
                allowed_hosts: hosts
                    .into_iter()
                    .map(|h| HostPattern::parse(h).unwrap())
                    .collect(),
            }
        }

        async fn run(allowlist: HostHeaderAllowList, host_header: &str) -> u16 {
            let inner = tower::service_fn(|_req: Request<()>| async {
                Ok::<_, Infallible>(Response::new(Vec::<u8>::new()))
            });
            let svc = allowlist.layer(inner);
            let req = Request::builder()
                .uri("/")
                .header(HOST, host_header)
                .body(())
                .unwrap();
            svc.oneshot(req).await.unwrap().status().as_u16()
        }

        #[tokio::test]
        async fn lan_mode_admits_loopback_without_explicit_entry() {
            let allowlist = HostHeaderAllowList::from_config(&cfg(DeploymentMode::Lan, vec![]));
            assert_eq!(run(allowlist, "127.0.0.1").await, 200);
        }

        #[tokio::test]
        async fn internet_mode_rejects_loopback_without_explicit_entry() {
            // Defense-in-depth: public deployments must not admit `Host:
            // 127.0.0.1` by default, or a future handler that privileges
            // loopback ("admin probe from localhost") becomes a bypass.
            let allowlist = HostHeaderAllowList::from_config(&cfg(
                DeploymentMode::Internet,
                vec!["shop.example.com"],
            ));
            assert_eq!(run(allowlist, "127.0.0.1").await, 403);
        }

        #[tokio::test]
        async fn reverse_proxy_mode_rejects_loopback_without_explicit_entry() {
            let allowlist = HostHeaderAllowList::from_config(&cfg(
                DeploymentMode::ReverseProxy,
                vec!["shop.example.com"],
            ));
            assert_eq!(run(allowlist, "127.0.0.1").await, 403);
        }

        #[tokio::test]
        async fn internet_mode_admits_loopback_when_explicitly_configured() {
            let allowlist = HostHeaderAllowList::from_config(&cfg(
                DeploymentMode::Internet,
                vec!["127.0.0.1", "shop.example.com"],
            ));
            assert_eq!(run(allowlist.clone(), "127.0.0.1").await, 200);
            assert_eq!(run(allowlist, "shop.example.com").await, 200);
        }
    }
}
