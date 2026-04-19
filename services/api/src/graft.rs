//! `MokumoApp: Graft` — the Mokumo application fused to the kikan engine.
//!
//! Wave A.1 scope: materialize the `Graft` impl with `type AppState =
//! MokumoAppState`, taking ownership of the per-profile migration set. The
//! full rewire of `build_app`/`build_app_with_shutdown` through
//! `Engine::<MokumoApp>::build_router` is deferred: `build_router` bakes in a
//! fixed tower stack (session → trace → host allow-list) that does not
//! accommodate `axum-login`'s `AuthManagerLayerBuilder`, `ProfileDbMiddleware`,
//! or `security_headers` — services/api still needs the hand-rolled stack
//! until the layer-ordering design pass lands. See the Wave A.1 notes in
//! `/ops/workspace/mokumo/mokumo-20260417-kikan-stages-4-6/shape-plan-v2.md`.
//!
//! Until then, `build_state` and `data_plane_routes` are intentionally
//! unimplemented — only `migrations()` is exercised (by the
//! `schema_equivalence` test and the per-profile migration runner via
//! `Engine::run_migrations`).

use kikan::migrations::conn::MigrationConn;
use kikan::{EngineContext, EngineError, Graft, GraftId, Migration, MigrationRef, MigrationTarget};
use mokumo_shop::migrations::Migrator;
use sea_orm_migration::MigratorTrait;
use sea_orm_migration::sea_orm::DbErr;

use crate::SharedState;

const MOKUMO_GRAFT_ID: GraftId = GraftId::new("mokumo");

pub struct MokumoApp;

impl Graft for MokumoApp {
    // `MokumoAppState` is always consumed behind `Arc` (`SharedState`) —
    // FromRef/Clone on per-request extraction requires cheap clone, which
    // `Arc<T>` provides without forcing Clone on every field.
    type AppState = SharedState;

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

        // login_lockout (index 6) ALTER TABLEs the `users` table, which is
        // now owned by kikan's platform graft.  Declare the cross-graft dep.
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

                // Positional chain: each migration depends on the previous.
                if i > 0 {
                    deps.push(MigrationRef {
                        graft: MOKUMO_GRAFT_ID,
                        name: names[i - 1],
                    });
                }

                // login_lockout also depends on users_and_roles (kikan).
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

    async fn build_state(&self, _ctx: &EngineContext) -> Result<Self::AppState, EngineError> {
        // Deferred: MokumoAppState construction requires extras that do not
        // live on EngineContext (rate limiters, reset_pins, recovery_dir,
        // mdns_status, local_ip watch, setup flags, WS manager, shutdown
        // token, DB pools keyed by profile). Wave A.1 leaves services/api's
        // `build_app_inner` as the sole construction site; the `build_state`
        // rewire is tracked with the `Engine::build_router` layer-order
        // design pass.
        Err(EngineError::Boot(
            "MokumoApp::build_state is not wired yet; services/api::build_app \
             constructs MokumoAppState directly (Wave A.1 scope cut)"
                .to_string(),
        ))
    }

    fn data_plane_routes(_state: &Self::AppState) -> axum::Router<Self::AppState> {
        // Deferred: the production router composition lives in
        // `services/api::build_app_inner` and layers `AuthManagerLayerBuilder`,
        // `ProfileDbMiddleware`, `security_headers`, and the host allow-list
        // that `Engine::build_router` does not currently provide hooks for.
        // Returning an empty router keeps the trait satisfied without
        // pretending to own the routing seam.
        axum::Router::new()
    }

    fn on_backup_created(
        &self,
        db_path: &std::path::Path,
        backup_path: &std::path::Path,
    ) -> Result<(), String> {
        mokumo_shop::lifecycle::copy_logo_to_backup(db_path, backup_path);
        Ok(())
    }

    fn on_post_restore(
        &self,
        db_path: &std::path::Path,
        backup_path: &std::path::Path,
    ) -> Result<(), String> {
        mokumo_shop::lifecycle::restore_logo_from_backup(db_path, backup_path);
        Ok(())
    }

    fn on_post_reset_db(
        &self,
        profile_dir: &std::path::Path,
        _recovery_dir: &std::path::Path,
    ) -> Result<(), String> {
        mokumo_shop::lifecycle::cleanup_domain_artifacts(profile_dir);
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
