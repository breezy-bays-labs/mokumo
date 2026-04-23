//! Data-plane router composition — the eight-layer middleware stack that
//! wraps a graft's domain routes, the `/api/**` typed-JSON-404 catch-all,
//! and the optional SPA fallback.
//!
//! Layer order (outermost → innermost), as applied by [`compose_router`]:
//!
//! 1. **HostHeaderAllowList** — reject disallowed Host headers first.
//! 2. **ForwardedLayer** — trust or strip `X-Forwarded-*`.
//! 3. **PerIpRateLimiterLayer** — per-IP global limit (non-Lan).
//! 4. **SecurityHeaders** — CSP, X-Frame-Options, etc.
//! 5. **TraceLayer** — request/response tracing.
//! 6. **AuthManagerLayer** — session + auth backend (axum-login).
//! 7. **CsrfLayer** — double-submit cookie + Origin check (non-Lan).
//! 8. **ProfileDbMiddleware** — inject per-request `ProfileDb` based on the
//!    authenticated session's profile. Uses `from_fn_with_state` to bind
//!    [`PlatformState`] independently of the graft's `AppState`.
//!
//! Axum applies the last `.layer()` as the outermost wrap.

use std::collections::HashMap;
use std::hash::Hash;
use std::marker::PhantomData;
use std::str::FromStr;
use std::sync::Arc;

use axum::Router;
use axum_login::AuthManagerLayerBuilder;
use sea_orm::DatabaseConnection;
use tower_http::trace::TraceLayer;

use super::config::DataPlaneConfig;
use super::csrf_layer::CsrfLayer;
use super::forwarded_layer::ForwardedLayer;
use super::rate_limiter_layer::{PerIpRateLimit, PerIpRateLimiterLayer};
use super::session_layer::session_layer_for_mode;
use super::spa::SpaSource;
use crate::auth::Backend;
use crate::engine::Sessions;
use crate::middleware::host_allowlist::HostHeaderAllowList;
use crate::middleware::security_headers;
use crate::platform_state::PlatformState;
use crate::profile_db::profile_db_middleware;

/// Inputs for [`compose_router`].
///
/// `state` is consumed by `with_state`; `platform` is cloned once for the
/// `from_fn_with_state` binding on the profile-db middleware. Everything
/// else is borrowed for the duration of the call.
pub(crate) struct ComposeInputs<'a, S, K> {
    pub routes: Router<S>,
    pub state: S,
    pub platform: PlatformState,
    pub sessions: &'a Sessions,
    pub config: &'a DataPlaneConfig,
    pub spa_source: Option<&'a dyn SpaSource>,
    pub _profile_kind: PhantomData<K>,
}

/// Wrap `inputs.routes` with the eight-layer middleware stack and bind
/// `inputs.state`. See module-level docs for layer ordering and per-mode
/// behavior.
///
/// Bounds on `K` mirror [`crate::graft::Graft::ProfileKind`] — the function
/// reconstructs `K` from boot-validated profile dir names, so the
/// `expect`s below encode invariants enforced in [`crate::Engine::boot`].
pub(crate) fn compose_router<S, K>(inputs: ComposeInputs<'_, S, K>) -> Router
where
    S: Clone + Send + Sync + 'static,
    K: Copy
        + Eq
        + Hash
        + Send
        + Sync
        + 'static
        + std::fmt::Display
        + std::fmt::Debug
        + FromStr<Err = String>
        + serde::Serialize
        + serde::de::DeserializeOwned,
{
    let ComposeInputs {
        routes,
        state,
        platform,
        sessions,
        config,
        spa_source,
        _profile_kind,
    } = inputs;

    // Auth backend dispatches by compound user ID across every profile pool
    // the graft declared. Every dir name in `profile_dir_names` round-trips
    // through `K::from_str` by construction — `Engine::boot` verifies that
    // invariant for every kind, so an `Err` here would signal bookkeeping
    // drift, not a runtime surprise.
    let mut pool_map: HashMap<K, DatabaseConnection> = HashMap::new();
    for dir in platform.profile_dir_names.iter() {
        let Some(pool) = platform.db_for(dir.as_str()) else {
            continue;
        };
        let kind = K::from_str(dir.as_str())
            .expect("boot invariant: profile dir round-trips through K::from_str");
        pool_map.insert(kind, pool.clone());
    }
    let auth_kind = K::from_str(platform.auth_profile_kind_dir.as_str())
        .expect("boot invariant: auth profile kind dir round-trips through K::from_str");
    let backend = Backend::<K>::new(Arc::new(pool_map), auth_kind);
    let auth_layer = AuthManagerLayerBuilder::new(
        backend,
        session_layer_for_mode(sessions, config.deployment_mode),
    )
    .build();

    let csrf_layer = CsrfLayer::for_mode(config.deployment_mode, config.allowed_origins.clone());
    let rate_limit_layer =
        PerIpRateLimiterLayer::for_mode(config.deployment_mode, PerIpRateLimit::default());
    let forwarded_layer = ForwardedLayer::for_mode(config.deployment_mode);
    let host_allowlist = HostHeaderAllowList::from_config(config);

    mount_spa_fallback(routes, spa_source)
        .layer(axum::middleware::from_fn_with_state(
            platform,
            profile_db_middleware::<K>,
        ))
        .layer(csrf_layer)
        .layer(auth_layer)
        .layer(TraceLayer::new_for_http())
        .layer(axum::middleware::from_fn(security_headers::middleware))
        .layer(rate_limit_layer)
        .layer(forwarded_layer)
        .layer(host_allowlist)
        .with_state(state)
}

/// Mount the SPA fallback plus the `/api/**` typed-JSON-404 catch-all onto
/// the vertical's data-plane routes.
///
/// The SPA fallback serves the HTML shell for any non-`/api/**` path that
/// the graft did not handle, so SvelteKit's client-side router can take
/// over. Explicit `/api` and `/api/*rest` routes ahead of the fallback
/// keep unmatched API paths on the JSON error contract — axum matches
/// more-specific routes first, so concrete API endpoints like
/// `/api/health` still take precedence.
///
/// No SPA means no catch-all either: unmatched paths fall through to
/// axum's default 404, preserving the pre-SPA behavior for consumers
/// that never mount one (CLI tools, tests, API-only deployments).
fn mount_spa_fallback<S>(routes: Router<S>, spa: Option<&dyn SpaSource>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let Some(spa) = spa else {
        return routes;
    };
    routes
        .route("/api", axum::routing::any(api_not_found))
        .route("/api/{*rest}", axum::routing::any(api_not_found))
        .fallback_service(spa.router())
}

/// Typed JSON 404 handler for unmatched `/api/**` paths.
///
/// Installed as a catch-all in [`mount_spa_fallback`] only when a
/// [`SpaSource`] is mounted — otherwise unmatched API paths produce
/// Axum's default 404 (the pre-SPA behavior). `no-store` prevents
/// transient 404s from being cached by intermediaries.
async fn api_not_found() -> axum::response::Response {
    use axum::Json;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    let body = kikan_types::error::ErrorBody {
        code: kikan_types::error::ErrorCode::NotFound,
        message: "No API route matches this path".into(),
        details: None,
    };
    (
        StatusCode::NOT_FOUND,
        [(axum::http::header::CACHE_CONTROL, "no-store")],
        Json(body),
    )
        .into_response()
}

#[cfg(test)]
mod compose_router_tests {
    use super::{ComposeInputs, compose_router};
    use crate::data_plane::config::DataPlaneConfig;
    use crate::engine::Sessions;
    use crate::platform_state::{MdnsStatus, PlatformState, ProfileDbInitializer};
    use crate::tenancy::ProfileDirName;
    use axum::body::Body;
    use http::{Request, header};
    use std::collections::HashMap;
    use std::future::Future;
    use std::marker::PhantomData;
    use std::path::PathBuf;
    use std::pin::Pin;
    use std::str::FromStr;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use tokio_util::sync::CancellationToken;
    use tower::ServiceExt;
    use tower_sessions_sqlx_store::SqliteStore;

    /// Single-variant profile kind. `Display` and `FromStr` are inverses
    /// by construction — the boot invariants the production `expect`s
    /// rely on hold trivially.
    #[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, serde::Serialize, serde::Deserialize)]
    struct TestProfile;

    impl std::fmt::Display for TestProfile {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("test")
        }
    }

    impl FromStr for TestProfile {
        type Err = String;
        fn from_str(s: &str) -> Result<Self, Self::Err> {
            if s == "test" {
                Ok(TestProfile)
            } else {
                Err(format!("unknown profile: {s}"))
            }
        }
    }

    struct UnreachableInitializer;
    impl ProfileDbInitializer for UnreachableInitializer {
        fn initialize<'a>(
            &'a self,
            _url: &'a str,
        ) -> Pin<
            Box<
                dyn Future<
                        Output = Result<sea_orm::DatabaseConnection, crate::db::DatabaseSetupError>,
                    > + Send
                    + 'a,
            >,
        > {
            Box::pin(async {
                unreachable!("test fixture: profile_db_initializer is never invoked here")
            })
        }
    }

    async fn fixture() -> (PlatformState, Sessions, DataPlaneConfig) {
        let dir = ProfileDirName::new("test".to_string()).unwrap();
        let pool = sea_orm::Database::connect("sqlite::memory:").await.unwrap();
        let mut pools: HashMap<ProfileDirName, sea_orm::DatabaseConnection> = HashMap::new();
        pools.insert(dir.clone(), pool);

        let platform = PlatformState {
            data_dir: PathBuf::from("/tmp"),
            db_filename: "test.db",
            pools: Arc::new(pools),
            active_profile: Arc::new(parking_lot::RwLock::new(dir.clone())),
            profile_dir_names: Arc::from(vec![dir.clone()]),
            requires_setup_by_dir: Arc::new(HashMap::new()),
            auth_profile_kind_dir: dir,
            shutdown: CancellationToken::new(),
            started_at: std::time::Instant::now(),
            mdns_status: MdnsStatus::shared(),
            demo_install_ok: Arc::new(AtomicBool::new(true)),
            is_first_launch: Arc::new(AtomicBool::new(false)),
            setup_completed: Arc::new(AtomicBool::new(true)),
            profile_db_initializer: Arc::new(UnreachableInitializer),
        };

        // Single-connection in-memory pool: migrate + session writes must
        // share the same `sqlite::memory:` database, otherwise SQLite hands
        // each connection its own private DB.
        let session_pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let store = SqliteStore::new(session_pool);
        store.migrate().await.unwrap();
        let sessions = Sessions::new(store);

        let config = DataPlaneConfig::lan_default("127.0.0.1:0".parse().unwrap());

        (platform, sessions, config)
    }

    /// Smoke test that locks in layer wiring: a request through the
    /// composed router must surface response-side effects from the
    /// middleware stack. We assert on the security-headers layer because
    /// its effect is observable on every response regardless of route
    /// match — if the layer order were broken (or the layer were dropped
    /// during a future refactor) the header would disappear.
    #[tokio::test]
    async fn compose_router_wires_security_headers_layer() {
        let (platform, sessions, config) = fixture().await;
        let routes: axum::Router<()> =
            axum::Router::new().route("/probe", axum::routing::get(|| async { "ok" }));

        let inputs = ComposeInputs::<(), TestProfile> {
            routes,
            state: (),
            platform,
            sessions: &sessions,
            config: &config,
            spa_source: None,
            _profile_kind: PhantomData,
        };
        let router = compose_router(inputs);

        let req = Request::builder()
            .uri("/probe")
            .header(header::HOST, "127.0.0.1")
            .body(Body::empty())
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();

        assert!(
            resp.headers().contains_key(header::X_FRAME_OPTIONS),
            "security_headers middleware did not run — layer wiring is broken; \
             observed headers: {:?}",
            resp.headers()
        );
    }
}

#[cfg(test)]
mod api_not_found_tests {
    use super::api_not_found;
    use axum::body::to_bytes;
    use axum::http::StatusCode;

    #[tokio::test]
    async fn returns_typed_json_404_with_no_store() {
        let response = api_not_found().await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            response
                .headers()
                .get(axum::http::header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some("no-store"),
            "transient API 404s must not be cached by intermediaries",
        );
        let bytes = to_bytes(response.into_body(), 1024).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["code"], "not_found");
        assert!(body["message"].as_str().unwrap().contains("No API route"));
    }
}
