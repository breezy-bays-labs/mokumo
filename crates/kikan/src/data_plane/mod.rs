//! Data-plane configuration, middleware, and session wiring.
//!
//! The data plane is the HTTP surface the SPA and external clients speak to
//! — as distinct from the control plane (admin UDS surface). Everything in
//! this module is engine-owned: verticals pick a [`DeploymentMode`] at boot
//! and supply allowed hosts/origins; they do not customize the middleware
//! itself.
//!
//! # Deployment modes
//!
//! Three postures ([`DeploymentMode`]), each with a distinct threat model:
//!
//! | Layer                | Lan                    | Internet                  | ReverseProxy              |
//! |----------------------|------------------------|---------------------------|---------------------------|
//! | Host allowlist       | loopback + mDNS hosts  | configured hosts only     | configured hosts only     |
//! | Cookie `Secure`      | `false` (HTTP on LAN)  | `true`                    | `true`                    |
//! | Cookie `SameSite`    | `Lax`                  | `Strict`                  | `Strict`                  |
//! | CSRF double-submit   | off                    | on                        | on                        |
//! | Per-IP rate limit    | off                    | on, fail-closed on no IP  | on, fail-closed on no IP  |
//! | Trust `X-Forwarded-*`| off (stripped)         | off (stripped)            | on; 400 on malformed      |
//! | mDNS registration    | on                     | off                       | off                       |
//!
//! Public-facing deployments (`Internet` / `ReverseProxy`) do NOT admit
//! loopback by default — operators who want a loopback health probe must
//! pass it explicitly via `--allowed-host 127.0.0.1`. Defense-in-depth
//! against a future handler that treats `Host: 127.0.0.1` as a privileged
//! local caller.
//!
//! # Middleware order
//!
//! Outermost → innermost, as applied in `engine::Engine::build_router`:
//!
//! 1. `HostHeaderAllowList` — reject disallowed Host headers before any
//!    other work.
//! 2. `forwarded_layer` — either trust or strip `X-Forwarded-For` /
//!    `X-Forwarded-Proto`, so downstream layers see the correct client IP.
//! 3. `rate_limiter_layer` — per-IP global limit (keys on the IP from #2).
//! 4. `security_headers` — CSP, `X-Frame-Options`, etc.
//! 5. `TraceLayer` — request/response tracing.
//! 6. `auth_layer` — session + axum-login backend.
//! 7. `csrf_layer` — double-submit cookie + Origin check (reads session
//!    from #6).
//! 8. `profile_db_middleware` — inject per-request `ProfileDb`.

pub mod bind;
pub mod config;
pub mod csrf_layer;
pub mod forwarded_layer;
pub mod rate_limiter_layer;
pub(crate) mod router;
pub mod session_layer;
pub mod spa;

pub use config::{DataPlaneConfig, DeploymentMode, HostPattern, HostPatternError};
