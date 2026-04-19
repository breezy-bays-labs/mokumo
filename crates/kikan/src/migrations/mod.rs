pub mod bootstrap;
pub mod conn;
pub mod dag;
pub mod platform;
pub mod runner;

use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GraftId(&'static str);

impl GraftId {
    pub const fn new(id: &'static str) -> Self {
        Self(id)
    }

    pub fn get(&self) -> &'static str {
        self.0
    }
}

impl std::fmt::Display for GraftId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MigrationRef {
    pub graft: GraftId,
    pub name: &'static str,
}

impl std::fmt::Display for MigrationRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}::{}", self.graft, self.name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MigrationTarget {
    Meta = 0,
    Session = 1,
    PerProfile = 2,
    Backup = 3,
}

#[async_trait::async_trait]
pub trait Migration: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn graft_id(&self) -> GraftId;
    fn target(&self) -> MigrationTarget;
    fn dependencies(&self) -> Vec<MigrationRef>;
    async fn up(&self, conn: &conn::MigrationConn) -> Result<(), sea_orm::DbErr>;
}

pub(crate) fn collect_migrations(
    graft_migrations: Vec<Box<dyn Migration>>,
    subgraft_migrations: Vec<Vec<Box<dyn Migration>>>,
) -> Vec<Arc<dyn Migration>> {
    graft_migrations
        .into_iter()
        .chain(subgraft_migrations.into_iter().flatten())
        .map(Arc::from)
        .collect()
}
