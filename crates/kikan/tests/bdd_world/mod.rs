use cucumber::World;
use kikan_types::activity::ActivityEntryResponse;
use sqlx::SqlitePool;
use std::sync::Arc;

mod activity_visibility_steps;
mod migration_execution_steps;
mod migration_ordering_steps;

#[derive(World)]
#[world(init = Self::new)]
pub struct KikanWorld {
    pub migrations: Vec<Arc<dyn kikan::Migration>>,
    pub resolve_result: Option<Result<Vec<Arc<dyn kikan::Migration>>, kikan::DagError>>,
    pub runner_result: Option<Result<(), kikan::EngineError>>,
    pub db: Option<sea_orm::DatabaseConnection>,
    pub _tmp: Option<tempfile::TempDir>,
    pub migration_execution_log: Vec<String>,
    pub fk_disabled_during_batch: Option<bool>,
    pub fk_enabled_after_batch: Option<bool>,
    // activity_visibility fixtures
    pub activity_pool: Option<SqlitePool>,
    pub activity_tmp: Option<tempfile::TempDir>,
    pub activity_list: Vec<ActivityEntryResponse>,
    pub activity_total: i64,
}

impl std::fmt::Debug for KikanWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KikanWorld")
            .field("migration_count", &self.migrations.len())
            .field("has_resolve_result", &self.resolve_result.is_some())
            .field("has_runner_result", &self.runner_result.is_some())
            .field("has_db", &self.db.is_some())
            .field("activity_list_len", &self.activity_list.len())
            .finish()
    }
}

impl KikanWorld {
    async fn new() -> Self {
        Self {
            migrations: Vec::new(),
            resolve_result: None,
            runner_result: None,
            db: None,
            _tmp: None,
            migration_execution_log: Vec::new(),
            fk_disabled_during_batch: None,
            fk_enabled_after_batch: None,
            activity_pool: None,
            activity_tmp: None,
            activity_list: Vec::new(),
            activity_total: 0,
        }
    }
}
