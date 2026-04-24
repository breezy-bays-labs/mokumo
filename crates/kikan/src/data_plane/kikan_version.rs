//! `GET /api/kikan-version` — unauthenticated version endpoint.
//!
//! Exposes the engine's API-contract version, crate semver, build-time
//! git SHA, and per-database applied-migration name so the admin SPA
//! can detect engine/UI drift before login renders. Verticals mount
//! [`kikan_version_router`] alongside `/api/health` in their data-plane
//! route assembly.
//!
//! Endpoint invariants: no state beyond kikan's [`PlatformState`], no
//! vertical vocabulary in the response (keys are the opaque
//! `ProfileDirName` strings the graft declared), and no auth layer.

use std::collections::BTreeMap;

use axum::Router;
use axum::extract::State;
use axum::response::Json;
use axum::routing::get;

use kikan_types::{API_VERSION, KikanVersionResponse};

use crate::AppError;
use crate::platform_state::PlatformState;

/// Router contributing `/api/kikan-version`.
///
/// Caller composes this under its public route group (no auth), typically
/// peer to `/api/health`. The router is `Router<S>` and adapts to the
/// graft's `AppState` via `with_state(platform)`; the handler extracts
/// [`PlatformState`] directly so no per-graft wiring is required.
pub fn kikan_version_router<S>(platform: PlatformState) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/api/kikan-version", get(handler))
        .with_state(platform)
}

async fn handler(
    State(platform): State<PlatformState>,
) -> Result<Json<KikanVersionResponse>, AppError> {
    let mut schema_versions: BTreeMap<String, String> = BTreeMap::new();
    for dir in platform.profile_dir_names.iter() {
        let Some(pool) = platform.db_for(dir.as_str()) else {
            continue;
        };
        let latest = latest_applied_migration(pool).await.unwrap_or_default();
        schema_versions.insert(dir.as_str().to_string(), latest);
    }

    Ok(Json(KikanVersionResponse {
        api_version: API_VERSION.to_string(),
        engine_version: env!("CARGO_PKG_VERSION").to_string(),
        engine_commit: env!("KIKAN_ENGINE_COMMIT").to_string(),
        schema_versions,
    }))
}

/// Highest applied migration version in the pool's `seaql_migrations`
/// table, or `None` when the table is empty or does not exist.
///
/// Uses `MAX(version)` to match sea-orm-migration's lexicographic
/// ordering of timestamp-prefixed migration names (`m20260404_000000_*`).
/// Any SQL error is treated as "unknown" — the endpoint must never
/// surface a DB failure to the unauthenticated caller.
async fn latest_applied_migration(db: &sea_orm::DatabaseConnection) -> Option<String> {
    use sea_orm::{FromQueryResult, Statement};

    #[derive(FromQueryResult)]
    struct Row {
        version: Option<String>,
    }

    let row = Row::find_by_statement(Statement::from_string(
        db.get_database_backend(),
        "SELECT MAX(version) AS version FROM seaql_migrations",
    ))
    .one(db)
    .await
    .ok()
    .flatten()?;
    row.version.filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::{Request, StatusCode};
    use sea_orm::{ConnectionTrait, Database, DatabaseConnection};
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use tokio_util::sync::CancellationToken;
    use tower::ServiceExt;

    use crate::platform_state::{MdnsStatus, ProfileDbInitializer};
    use crate::tenancy::ProfileDirName;

    struct UnreachableInitializer;
    impl ProfileDbInitializer for UnreachableInitializer {
        fn initialize<'a>(
            &'a self,
            _url: &'a str,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<DatabaseConnection, crate::db::DatabaseSetupError>,
                    > + Send
                    + 'a,
            >,
        > {
            Box::pin(async { unreachable!("test fixture: profile_db_initializer not invoked") })
        }
    }

    async fn seed_pool(migrations: &[&str]) -> DatabaseConnection {
        let pool = Database::connect("sqlite::memory:").await.unwrap();
        pool.execute_unprepared(
            "CREATE TABLE seaql_migrations (version TEXT NOT NULL, applied_at BIGINT NOT NULL)",
        )
        .await
        .unwrap();
        for v in migrations {
            pool.execute_unprepared(&format!("INSERT INTO seaql_migrations VALUES ('{v}', 0)"))
                .await
                .unwrap();
        }
        pool
    }

    async fn fixture(kinds: &[(&str, &[&str])]) -> PlatformState {
        let mut pools: HashMap<ProfileDirName, DatabaseConnection> = HashMap::new();
        let mut dir_names: Vec<ProfileDirName> = Vec::new();
        for (dir, versions) in kinds {
            let d = ProfileDirName::new((*dir).to_string()).unwrap();
            pools.insert(d.clone(), seed_pool(versions).await);
            dir_names.push(d);
        }
        let auth_dir = dir_names[0].clone();
        let active = dir_names[0].clone();
        PlatformState {
            data_dir: PathBuf::from("/tmp"),
            db_filename: "test.db",
            pools: Arc::new(pools),
            active_profile: Arc::new(parking_lot::RwLock::new(active)),
            profile_dir_names: Arc::from(dir_names),
            requires_setup_by_dir: Arc::new(HashMap::new()),
            auth_profile_kind_dir: auth_dir,
            shutdown: CancellationToken::new(),
            started_at: std::time::Instant::now(),
            mdns_status: MdnsStatus::shared(),
            demo_install_ok: Arc::new(AtomicBool::new(true)),
            is_first_launch: Arc::new(AtomicBool::new(false)),
            setup_completed: Arc::new(AtomicBool::new(true)),
            profile_db_initializer: Arc::new(UnreachableInitializer),
        }
    }

    #[tokio::test]
    async fn returns_api_and_engine_version_plus_schema_map() {
        let platform = fixture(&[
            ("demo", &["m20260321_000000_init", "m20260324_000000_seq"]),
            ("production", &["m20260321_000000_init"]),
        ])
        .await;

        let app: Router = kikan_version_router::<()>(platform);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/kikan-version")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), 4096).await.unwrap();
        let body: KikanVersionResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body.api_version, API_VERSION);
        assert_eq!(body.engine_version, env!("CARGO_PKG_VERSION"));
        assert!(!body.engine_commit.is_empty());
        assert_eq!(
            body.schema_versions.get("demo").map(String::as_str),
            Some("m20260324_000000_seq"),
        );
        assert_eq!(
            body.schema_versions.get("production").map(String::as_str),
            Some("m20260321_000000_init"),
        );
    }

    #[tokio::test]
    async fn empty_seaql_migrations_yields_empty_string_entry() {
        let platform = fixture(&[("demo", &[])]).await;
        let app: Router = kikan_version_router::<()>(platform);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/kikan-version")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), 4096).await.unwrap();
        let body: KikanVersionResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            body.schema_versions.get("demo").map(String::as_str),
            Some("")
        );
    }
}
