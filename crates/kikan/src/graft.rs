use std::path::Path;

use crate::engine::EngineContext;
use crate::error::EngineError;
use crate::migrations::bootstrap::BootstrapMigrations;
use crate::migrations::platform::PlatformMigrations;
use crate::migrations::{GraftId, Migration};

#[trait_variant::make(Send)]
pub trait Graft: Sized + 'static {
    type AppState: Clone + Send + Sync + 'static;

    fn id() -> GraftId;
    fn migrations(&self) -> Vec<Box<dyn Migration>>;
    async fn build_state(&self, ctx: &EngineContext) -> Result<Self::AppState, EngineError>;
    fn data_plane_routes(state: &Self::AppState) -> axum::Router<Self::AppState>;

    // ── Lifecycle hooks (sync, default no-ops) ──────────────────────────

    /// Called after a backup archive has been created. Domain grafts can
    /// copy additional artifacts (e.g. logo files) into the backup.
    fn on_backup_created(&self, _db_path: &Path, _backup_path: &Path) -> Result<(), String> {
        Ok(())
    }

    /// Called before a restore operation begins. Domain grafts can validate
    /// or prepare domain-specific state before the database is replaced.
    fn on_pre_restore(&self, _db_path: &Path, _backup_path: &Path) -> Result<(), String> {
        Ok(())
    }

    /// Called after a restore operation completes. Domain grafts can restore
    /// additional artifacts (e.g. logo files) from the backup.
    fn on_post_restore(&self, _db_path: &Path, _backup_path: &Path) -> Result<(), String> {
        Ok(())
    }

    /// Called after a database reset. Domain grafts can clean up
    /// domain-specific artifacts from the profile directory.
    fn on_post_reset_db(&self, _profile_dir: &Path, _recovery_dir: &Path) -> Result<(), String> {
        Ok(())
    }

    /// Spawn domain-specific background tasks (e.g. periodic IP refresh,
    /// PIN sweep). Called once after state construction during boot.
    fn spawn_background_tasks(&self, _state: &Self::AppState) {}
}

#[async_trait::async_trait]
pub trait SubGraft: Send + Sync + 'static {
    fn id(&self) -> GraftId;
    fn migrations(&self) -> Vec<Box<dyn Migration>>;

    async fn on_ignite(&self) -> Result<(), EngineError> {
        Ok(())
    }
    async fn on_liftoff(&self) -> Result<(), EngineError> {
        Ok(())
    }
    async fn on_shutdown(&self) -> Result<(), EngineError> {
        Ok(())
    }
    async fn check(&self) -> Result<(), EngineError> {
        Ok(())
    }
}

pub struct SelfGraft;

#[async_trait::async_trait]
impl SubGraft for SelfGraft {
    fn id(&self) -> GraftId {
        BootstrapMigrations::graft_id()
    }

    fn migrations(&self) -> Vec<Box<dyn Migration>> {
        let mut migrations = BootstrapMigrations::migrations();
        migrations.extend(PlatformMigrations::migrations());
        migrations
    }
}
