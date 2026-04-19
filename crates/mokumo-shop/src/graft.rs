//! `MokumoApp: Graft` — the Mokumo application fused to the kikan engine.
//!
//! Owns the per-profile migration set and lifecycle hooks. `build_domain_state`
//! constructs `MokumoShopState`; `compose_state` assembles the full
//! `MokumoState` from platform + control-plane + domain slices.
//!
//! `data_plane_routes` is deferred to PR 3 — the production router
//! composition currently lives in `services/api::build_app_inner`.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use kikan::migrations::conn::MigrationConn;
use kikan::rate_limit::RateLimiter;
use kikan::{EngineContext, EngineError, Graft, GraftId, Migration, MigrationRef, MigrationTarget};
use sea_orm_migration::MigratorTrait;
use sea_orm_migration::sea_orm::DbErr;

use crate::migrations::Migrator;
use crate::state::{MokumoShopState, MokumoState, SharedMokumoState};
use crate::ws::ConnectionManager;

const MOKUMO_GRAFT_ID: GraftId = GraftId::new("mokumo");

pub struct MokumoApp;

impl Graft for MokumoApp {
    type AppState = SharedMokumoState;
    type DomainState = MokumoShopState;

    fn id() -> GraftId {
        MOKUMO_GRAFT_ID
    }

    fn migrations(&self) -> Vec<Box<dyn Migration>> {
        let seaorm_migrations = Migrator::migrations();
        let names: Vec<&'static str> = vec![
            "m20260321_000000_init",
            "m20260322_000000_settings",
            "m20260324_000000_number_sequences",
            "m20260324_000001_customers_and_activity",
            "m20260326_000000_customers_deleted_at_index",
            "m20260404_000000_set_pragmas",
            "m20260416_000000_login_lockout",
            "m20260418_000000_activity_log_composite_index",
        ];

        const KIKAN_GRAFT_ID: GraftId = GraftId::new("kikan");
        let login_lockout_cross_graft_dep = MigrationRef {
            graft: KIKAN_GRAFT_ID,
            name: "m20260327_000000_users_and_roles",
        };

        seaorm_migrations
            .into_iter()
            .enumerate()
            .map(|(i, m)| {
                let mut deps: Vec<MigrationRef> = Vec::new();

                if i > 0 {
                    deps.push(MigrationRef {
                        graft: MOKUMO_GRAFT_ID,
                        name: names[i - 1],
                    });
                }

                if names[i] == "m20260416_000000_login_lockout" {
                    deps.push(login_lockout_cross_graft_dep.clone());
                }

                Box::new(BridgedSeaOrmMigration {
                    inner: m,
                    name: names[i],
                    deps,
                }) as Box<dyn Migration>
            })
            .collect()
    }

    async fn build_domain_state(
        &self,
        _ctx: &EngineContext,
    ) -> Result<Self::DomainState, EngineError> {
        let (_, local_ip_rx) = tokio::sync::watch::channel(None);

        Ok(MokumoShopState {
            ws: Arc::new(ConnectionManager::new(64)),
            local_ip: local_ip_rx,
            restore_in_progress: Arc::new(AtomicBool::new(false)),
            restore_limiter: Arc::new(RateLimiter::new(5, std::time::Duration::from_secs(3600))),
            #[cfg(debug_assertions)]
            ws_ping_ms: None,
        })
    }

    fn compose_state(
        _platform: kikan::PlatformState,
        control_plane: kikan::ControlPlaneState,
        domain: Self::DomainState,
    ) -> Self::AppState {
        Arc::new(MokumoState {
            control_plane,
            domain,
        })
    }

    fn platform_state(state: &Self::AppState) -> &kikan::PlatformState {
        &state.control_plane.platform
    }

    fn control_plane_state(state: &Self::AppState) -> &kikan::ControlPlaneState {
        &state.control_plane
    }

    fn data_plane_routes(_state: &Self::AppState) -> axum::Router<Self::AppState> {
        // Deferred to PR 3: production router lives in services/api::build_app_inner.
        axum::Router::new()
    }

    fn on_backup_created(
        &self,
        db_path: &std::path::Path,
        backup_path: &std::path::Path,
    ) -> Result<(), String> {
        crate::lifecycle::copy_logo_to_backup(db_path, backup_path);
        Ok(())
    }

    fn on_post_restore(
        &self,
        db_path: &std::path::Path,
        backup_path: &std::path::Path,
    ) -> Result<(), String> {
        crate::lifecycle::restore_logo_from_backup(db_path, backup_path);
        Ok(())
    }

    fn on_post_reset_db(
        &self,
        profile_dir: &std::path::Path,
        _recovery_dir: &std::path::Path,
    ) -> Result<(), String> {
        crate::lifecycle::cleanup_domain_artifacts(profile_dir);
        Ok(())
    }
}

struct BridgedSeaOrmMigration {
    inner: Box<dyn sea_orm_migration::MigrationTrait + Send + Sync>,
    name: &'static str,
    deps: Vec<MigrationRef>,
}

#[async_trait::async_trait]
impl Migration for BridgedSeaOrmMigration {
    fn name(&self) -> &'static str {
        self.name
    }

    fn graft_id(&self) -> GraftId {
        MOKUMO_GRAFT_ID
    }

    fn target(&self) -> MigrationTarget {
        MigrationTarget::PerProfile
    }

    fn dependencies(&self) -> Vec<MigrationRef> {
        self.deps.clone()
    }

    async fn up(&self, conn: &MigrationConn) -> Result<(), DbErr> {
        let manager = conn.schema_manager();
        self.inner.up(&manager).await
    }
}
