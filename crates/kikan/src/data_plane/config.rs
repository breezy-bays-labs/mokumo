//! [`DataPlaneConfig`] — per-deployment configuration consumed by data-plane
//! middleware. Engine-owned; verticals pick a mode at boot and supply the
//! allowed hosts/origins.

use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;

use http::HeaderValue;

/// Deployment posture per ADR `adr-kikan-deployment-modes.md`.
///
/// Picks the middleware matrix documented at the [`crate::data_plane`] module
/// level. `Internet` assumes kikan terminates TLS itself; `ReverseProxy`
/// assumes a proxy in front of kikan handles TLS and supplies
/// `X-Forwarded-For` / `X-Forwarded-Proto`.
#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum DeploymentMode {
    /// Private network, HTTP, trusts the LAN. mDNS discovery on.
    #[default]
    Lan,
    /// Public-facing, HTTPS expected at the socket. CSRF + per-IP limits on.
    Internet,
    /// Behind a trusted reverse proxy. `X-Forwarded-*` trusted; CSRF + per-IP
    /// limits on.
    ReverseProxy,
}

impl DeploymentMode {
    /// Cookies are `Secure`-flagged (HTTPS required) in non-Lan modes.
    pub fn cookies_require_secure(self) -> bool {
        !matches!(self, Self::Lan)
    }

    /// CSRF double-submit + Origin check is enforced in non-Lan modes.
    pub fn csrf_enabled(self) -> bool {
        !matches!(self, Self::Lan)
    }

    /// Per-IP rate limiter is engaged in non-Lan modes. LAN relies on the
    /// per-email / per-user limiters in the control plane.
    pub fn rate_limit_enabled(self) -> bool {
        !matches!(self, Self::Lan)
    }

    /// `X-Forwarded-For` / `X-Forwarded-Proto` are trusted only when a
    /// reverse proxy is in front.
    pub fn trust_forwarded(self) -> bool {
        matches!(self, Self::ReverseProxy)
    }

    /// mDNS service registration runs only in LAN posture.
    pub fn mdns_enabled(self) -> bool {
        matches!(self, Self::Lan)
    }
}

impl std::fmt::Display for DeploymentMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Lan => "lan",
            Self::Internet => "internet",
            Self::ReverseProxy => "reverse-proxy",
        })
    }
}

impl FromStr for DeploymentMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "lan" => Ok(Self::Lan),
            "internet" => Ok(Self::Internet),
            "reverse-proxy" | "reverse_proxy" => Ok(Self::ReverseProxy),
            other => Err(format!(
                "unknown deployment mode `{other}`; expected one of: lan, internet, reverse-proxy"
            )),
        }
    }
}

/// Per-deployment configuration consumed by data-plane middleware.
///
/// Engine-owned: verticals pick a [`DeploymentMode`] at boot and supply the
/// allowed hosts/origins; they do not customize the middleware itself.
///
/// Construct with [`DataPlaneConfig::new`] (or [`DataPlaneConfig::lan_default`]
/// for the zero-config LAN posture). The private `_priv` field blocks
/// struct-literal construction from outside this module so the non-Lan
/// non-empty-hosts/origins invariant is enforced at the type boundary —
/// stronger than `#[non_exhaustive]`, which would still allow in-crate
/// struct literals from the middleware test modules.
#[allow(clippy::manual_non_exhaustive)]
#[derive(Debug, Clone)]
pub struct DataPlaneConfig {
    pub deployment_mode: DeploymentMode,
    pub bind_addr: SocketAddr,
    /// Origins permitted for CSRF Origin-header validation (non-Lan modes).
    pub allowed_origins: Vec<HeaderValue>,
    /// Host header allowlist. In Lan mode, loopback + mDNS-derived hosts are
    /// added on top of whatever the caller supplies.
    pub allowed_hosts: Vec<HostPattern>,
    #[allow(dead_code)]
    _priv: (),
}

/// Reasons [`DataPlaneConfig::new`] rejects its inputs.
///
/// Non-Lan deployment modes (`Internet`, `ReverseProxy`) require at least
/// one allowed host pattern AND at least one allowed origin, otherwise the
/// server would 400 every POST (CSRF origin check fails with an empty
/// allowlist; host-header middleware rejects unknown hosts).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("non-Lan mode `{mode}` requires at least one allowed_hosts entry")]
    EmptyAllowedHosts { mode: DeploymentMode },
    #[error("non-Lan mode `{mode}` requires at least one allowed_origins entry")]
    EmptyAllowedOrigins { mode: DeploymentMode },
}

impl DataPlaneConfig {
    /// Construct a validated [`DataPlaneConfig`].
    ///
    /// Rejects non-Lan modes with empty `allowed_hosts` or `allowed_origins`
    /// so library callers can't silently boot a server that 400s every POST.
    /// When both lists are empty for a non-Lan mode the hosts check fires
    /// first; see [`ConfigError`].
    pub fn new(
        deployment_mode: DeploymentMode,
        bind_addr: SocketAddr,
        allowed_hosts: Vec<HostPattern>,
        allowed_origins: Vec<HeaderValue>,
    ) -> Result<Self, ConfigError> {
        if !matches!(deployment_mode, DeploymentMode::Lan) {
            if allowed_hosts.is_empty() {
                return Err(ConfigError::EmptyAllowedHosts {
                    mode: deployment_mode,
                });
            }
            if allowed_origins.is_empty() {
                return Err(ConfigError::EmptyAllowedOrigins {
                    mode: deployment_mode,
                });
            }
        }
        Ok(Self {
            deployment_mode,
            bind_addr,
            allowed_hosts,
            allowed_origins,
            _priv: (),
        })
    }

    /// LAN-mode default: loopback bind, no explicit allowlist entries
    /// (loopback + mDNS are added by the host allowlist builder).
    pub fn lan_default(bind_addr: SocketAddr) -> Self {
        Self {
            deployment_mode: DeploymentMode::Lan,
            bind_addr,
            allowed_origins: Vec::new(),
            allowed_hosts: Vec::new(),
            _priv: (),
        }
    }
}

/// Validated Host-header pattern. Constructed via [`HostPattern::parse`];
/// stored as a normalized lowercase, port-free string so comparison
/// against the value emitted by [`crate::middleware::host_allowlist`]'s
/// `parse_host` is an exact-string match. Inputs that include a port are
/// rejected with [`HostPatternError::ContainsPort`] rather than stripped.
///
/// Wildcards are not supported — callers list every hostname explicitly.
/// Reasons a pattern is rejected surface as [`HostPatternError`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HostPattern(Arc<str>);

/// Reasons a [`HostPattern`] constructor rejects its input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostPatternError {
    Empty,
    ContainsWhitespace,
    ContainsNul,
    ContainsPathSeparator,
    ContainsPort,
    ContainsScheme,
    ContainsWildcard,
    NonAscii,
    /// Input is structurally invalid in a way none of the other variants
    /// describes (e.g. a bracketed IPv6 literal missing its closing bracket).
    /// `reason` is a static string naming the specific defect.
    Malformed {
        reason: &'static str,
    },
}

impl std::fmt::Display for HostPatternError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => f.write_str("host pattern is empty"),
            Self::ContainsWhitespace => f.write_str("host pattern contains whitespace"),
            Self::ContainsNul => f.write_str("host pattern contains NUL byte"),
            Self::ContainsPathSeparator => {
                f.write_str("host pattern contains path separator (/ or \\)")
            }
            Self::ContainsPort => {
                f.write_str("host pattern must not include a port (strip `:PORT`)")
            }
            Self::ContainsScheme => {
                f.write_str("host pattern must not include a scheme (strip `http(s)://`)")
            }
            Self::ContainsWildcard => f.write_str("host pattern must not include wildcards (`*`)"),
            Self::NonAscii => f.write_str("host pattern contains non-ASCII characters"),
            Self::Malformed { reason } => write!(f, "host pattern is malformed: {reason}"),
        }
    }
}

impl std::error::Error for HostPatternError {}

impl HostPattern {
    /// Parse and validate a host string. Normalizes to lowercase.
    ///
    /// IPv6 literals must be passed with their brackets (`[::1]`) so the
    /// port-detection heuristic does not mistake the first `:` for a port
    /// separator.
    pub fn parse(input: impl AsRef<str>) -> Result<Self, HostPatternError> {
        let raw = input.as_ref();
        if raw.is_empty() {
            return Err(HostPatternError::Empty);
        }
        if !raw.is_ascii() {
            return Err(HostPatternError::NonAscii);
        }
        if raw.contains(char::is_whitespace) {
            return Err(HostPatternError::ContainsWhitespace);
        }
        if raw.contains('\0') {
            return Err(HostPatternError::ContainsNul);
        }
        if raw.contains("://") {
            return Err(HostPatternError::ContainsScheme);
        }
        if raw.contains('/') || raw.contains('\\') {
            return Err(HostPatternError::ContainsPathSeparator);
        }
        if raw.contains('*') {
            return Err(HostPatternError::ContainsWildcard);
        }

        let lower = raw.to_ascii_lowercase();

        // Port rejection: IPv6 literals keep their brackets; for everything
        // else, a `:` means a port was included.
        let has_port = if lower.starts_with('[') {
            match lower.find(']') {
                // `[::1]` — no port. `[::1]:6565` — has port.
                Some(idx) => idx + 1 < lower.len(),
                None => {
                    return Err(HostPatternError::Malformed {
                        reason: "unclosed bracket",
                    });
                }
            }
        } else {
            lower.contains(':')
        };
        if has_port {
            return Err(HostPatternError::ContainsPort);
        }

        Ok(Self(Arc::from(lower)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Loopback IPv4 (`127.0.0.1`), unconditionally valid.
    pub fn loopback_v4() -> Self {
        Self(Arc::from("127.0.0.1"))
    }

    /// Loopback IPv6 (`[::1]`), unconditionally valid.
    pub fn loopback_v6() -> Self {
        Self(Arc::from("[::1]"))
    }

    /// `localhost`, unconditionally valid.
    pub fn localhost() -> Self {
        Self(Arc::from("localhost"))
    }
}

impl FromStr for HostPattern {
    type Err = HostPatternError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_round_trips_through_serde() {
        for mode in [
            DeploymentMode::Lan,
            DeploymentMode::Internet,
            DeploymentMode::ReverseProxy,
        ] {
            let json = serde_json::to_string(&mode).unwrap();
            let parsed: DeploymentMode = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, mode);
        }
    }

    #[test]
    fn mode_round_trips_through_display_fromstr() {
        for mode in [
            DeploymentMode::Lan,
            DeploymentMode::Internet,
            DeploymentMode::ReverseProxy,
        ] {
            let s = mode.to_string();
            let parsed: DeploymentMode = s.parse().unwrap();
            assert_eq!(parsed, mode);
        }
    }

    #[test]
    fn mode_capability_flags() {
        assert!(!DeploymentMode::Lan.csrf_enabled());
        assert!(DeploymentMode::Internet.csrf_enabled());
        assert!(DeploymentMode::ReverseProxy.csrf_enabled());

        assert!(!DeploymentMode::Lan.trust_forwarded());
        assert!(!DeploymentMode::Internet.trust_forwarded());
        assert!(DeploymentMode::ReverseProxy.trust_forwarded());

        assert!(DeploymentMode::Lan.mdns_enabled());
        assert!(!DeploymentMode::Internet.mdns_enabled());
        assert!(!DeploymentMode::ReverseProxy.mdns_enabled());

        assert!(!DeploymentMode::Lan.cookies_require_secure());
        assert!(DeploymentMode::Internet.cookies_require_secure());
    }

    #[test]
    fn host_pattern_accepts_hostname() {
        let p = HostPattern::parse("shop.example.com").unwrap();
        assert_eq!(p.as_str(), "shop.example.com");
    }

    #[test]
    fn host_pattern_lowercases() {
        assert_eq!(
            HostPattern::parse("Shop.Example.COM").unwrap().as_str(),
            "shop.example.com"
        );
    }

    #[test]
    fn host_pattern_accepts_ipv4_loopback() {
        assert_eq!(
            HostPattern::parse("127.0.0.1").unwrap().as_str(),
            "127.0.0.1"
        );
    }

    #[test]
    fn host_pattern_accepts_bracketed_ipv6() {
        assert_eq!(HostPattern::parse("[::1]").unwrap().as_str(), "[::1]");
    }

    #[test]
    fn host_pattern_rejects_empty() {
        assert_eq!(HostPattern::parse(""), Err(HostPatternError::Empty));
    }

    #[test]
    fn host_pattern_rejects_whitespace() {
        assert_eq!(
            HostPattern::parse("shop example.com"),
            Err(HostPatternError::ContainsWhitespace)
        );
        assert_eq!(
            HostPattern::parse(" shop.example.com"),
            Err(HostPatternError::ContainsWhitespace)
        );
    }

    #[test]
    fn host_pattern_rejects_nul() {
        assert_eq!(
            HostPattern::parse("shop\0.example.com"),
            Err(HostPatternError::ContainsNul)
        );
    }

    #[test]
    fn host_pattern_rejects_path_separator() {
        assert_eq!(
            HostPattern::parse("shop.example.com/admin"),
            Err(HostPatternError::ContainsPathSeparator)
        );
        assert_eq!(
            HostPattern::parse("shop\\admin"),
            Err(HostPatternError::ContainsPathSeparator)
        );
    }

    #[test]
    fn host_pattern_rejects_scheme() {
        assert_eq!(
            HostPattern::parse("https://shop.example.com"),
            Err(HostPatternError::ContainsScheme)
        );
    }

    #[test]
    fn host_pattern_rejects_port() {
        assert_eq!(
            HostPattern::parse("shop.example.com:443"),
            Err(HostPatternError::ContainsPort)
        );
        assert_eq!(
            HostPattern::parse("127.0.0.1:6565"),
            Err(HostPatternError::ContainsPort)
        );
        assert_eq!(
            HostPattern::parse("[::1]:6565"),
            Err(HostPatternError::ContainsPort)
        );
    }

    #[test]
    fn host_pattern_rejects_wildcard() {
        assert_eq!(
            HostPattern::parse("*.shop.example.com"),
            Err(HostPatternError::ContainsWildcard)
        );
    }

    #[test]
    fn host_pattern_rejects_non_ascii() {
        assert_eq!(
            HostPattern::parse("shöp.example.com"),
            Err(HostPatternError::NonAscii)
        );
    }

    #[test]
    fn host_pattern_rejects_unclosed_ipv6_bracket_as_malformed() {
        // The unclosed-bracket case is structurally invalid but contains no
        // path separator — it must not borrow `ContainsPathSeparator`'s name.
        assert_eq!(
            HostPattern::parse("[::1"),
            Err(HostPatternError::Malformed {
                reason: "unclosed bracket"
            })
        );
    }

    #[test]
    fn host_pattern_malformed_display_includes_reason() {
        let err = HostPatternError::Malformed {
            reason: "unclosed bracket",
        };
        assert_eq!(
            err.to_string(),
            "host pattern is malformed: unclosed bracket"
        );
    }

    fn test_bind() -> SocketAddr {
        "127.0.0.1:0".parse().unwrap()
    }

    fn valid_hosts() -> Vec<HostPattern> {
        vec![HostPattern::parse("shop.example.com").unwrap()]
    }

    fn valid_origins() -> Vec<HeaderValue> {
        vec!["https://shop.example.com".parse().unwrap()]
    }

    #[test]
    fn new_rejects_internet_empty_hosts() {
        let err = DataPlaneConfig::new(
            DeploymentMode::Internet,
            test_bind(),
            Vec::new(),
            valid_origins(),
        )
        .unwrap_err();
        assert_eq!(
            err,
            ConfigError::EmptyAllowedHosts {
                mode: DeploymentMode::Internet
            }
        );
    }

    #[test]
    fn new_rejects_internet_empty_origins() {
        let err = DataPlaneConfig::new(
            DeploymentMode::Internet,
            test_bind(),
            valid_hosts(),
            Vec::new(),
        )
        .unwrap_err();
        assert_eq!(
            err,
            ConfigError::EmptyAllowedOrigins {
                mode: DeploymentMode::Internet
            }
        );
    }

    #[test]
    fn new_rejects_reverse_proxy_empty_hosts() {
        let err = DataPlaneConfig::new(
            DeploymentMode::ReverseProxy,
            test_bind(),
            Vec::new(),
            valid_origins(),
        )
        .unwrap_err();
        assert_eq!(
            err,
            ConfigError::EmptyAllowedHosts {
                mode: DeploymentMode::ReverseProxy
            }
        );
    }

    #[test]
    fn new_rejects_reverse_proxy_empty_origins() {
        let err = DataPlaneConfig::new(
            DeploymentMode::ReverseProxy,
            test_bind(),
            valid_hosts(),
            Vec::new(),
        )
        .unwrap_err();
        assert_eq!(
            err,
            ConfigError::EmptyAllowedOrigins {
                mode: DeploymentMode::ReverseProxy
            }
        );
    }

    #[test]
    fn new_reports_hosts_before_origins_when_both_empty() {
        // Short-circuit order is part of the contract: hosts before origins.
        let err = DataPlaneConfig::new(
            DeploymentMode::Internet,
            test_bind(),
            Vec::new(),
            Vec::new(),
        )
        .unwrap_err();
        assert!(matches!(err, ConfigError::EmptyAllowedHosts { .. }));
    }

    #[test]
    fn new_accepts_lan_empty_hosts_and_origins() {
        let cfg =
            DataPlaneConfig::new(DeploymentMode::Lan, test_bind(), Vec::new(), Vec::new()).unwrap();
        assert_eq!(cfg.deployment_mode, DeploymentMode::Lan);
        assert!(cfg.allowed_hosts.is_empty());
        assert!(cfg.allowed_origins.is_empty());
    }

    #[test]
    fn new_accepts_lan_with_entries() {
        let cfg = DataPlaneConfig::new(
            DeploymentMode::Lan,
            test_bind(),
            valid_hosts(),
            valid_origins(),
        )
        .unwrap();
        assert_eq!(cfg.allowed_hosts.len(), 1);
        assert_eq!(cfg.allowed_origins.len(), 1);
    }

    #[test]
    fn new_accepts_internet_with_entries() {
        let cfg = DataPlaneConfig::new(
            DeploymentMode::Internet,
            test_bind(),
            valid_hosts(),
            valid_origins(),
        )
        .unwrap();
        assert_eq!(cfg.deployment_mode, DeploymentMode::Internet);
    }

    #[test]
    fn new_accepts_reverse_proxy_with_entries() {
        let cfg = DataPlaneConfig::new(
            DeploymentMode::ReverseProxy,
            test_bind(),
            valid_hosts(),
            valid_origins(),
        )
        .unwrap();
        assert_eq!(cfg.deployment_mode, DeploymentMode::ReverseProxy);
    }
}
