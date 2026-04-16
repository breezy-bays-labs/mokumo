use kikan::migrations::conn::MigrationConn;
use kikan::{
    BootConfig, Engine, EngineError, Graft, GraftId, Migration, MigrationRef, MigrationTarget,
    Tenancy,
};
use std::sync::Arc;

pub struct StubGraft {
    migrations: Vec<Box<dyn Migration>>,
}

impl StubGraft {
    pub fn new(migrations: Vec<Box<dyn Migration>>) -> Self {
        Self { migrations }
    }

    pub fn diamond() -> Self {
        Self::new(vec![
            make_migration("A", vec![], MigrationTarget::PerProfile),
            make_migration("B", vec!["A"], MigrationTarget::PerProfile),
            make_migration("C", vec!["A"], MigrationTarget::PerProfile),
            make_migration("D", vec!["B", "C"], MigrationTarget::PerProfile),
        ])
    }
}

impl Graft for StubGraft {
    type AppState = ();

    fn id() -> GraftId {
        GraftId("stub")
    }

    fn migrations(&self) -> Vec<Box<dyn Migration>> {
        self.migrations
            .iter()
            .map(|m| -> Box<dyn Migration> {
                make_migration(
                    m.name(),
                    m.dependencies().iter().map(|d| d.name).collect(),
                    m.target(),
                )
            })
            .collect()
    }

    async fn build_state(&self, _tenancy: &Tenancy) -> Result<Self::AppState, EngineError> {
        Ok(())
    }

    async fn run(&self, _state: Self::AppState) -> Result<(), EngineError> {
        Ok(())
    }
}

pub fn make_migration(
    name: &'static str,
    deps: Vec<&'static str>,
    target: MigrationTarget,
) -> Box<dyn Migration> {
    Box::new(SimpleMigration {
        name,
        deps: deps.into_iter().collect(),
        target,
        sql: format!("CREATE TABLE IF NOT EXISTS test_{name} (id INTEGER PRIMARY KEY)"),
    })
}

pub fn failing_migration(name: &'static str, deps: Vec<&'static str>) -> Box<dyn Migration> {
    Box::new(SimpleMigration {
        name,
        deps,
        target: MigrationTarget::PerProfile,
        sql: "INVALID SQL STATEMENT HERE".to_string(),
    })
}

struct SimpleMigration {
    name: &'static str,
    deps: Vec<&'static str>,
    target: MigrationTarget,
    sql: String,
}

#[async_trait::async_trait]
impl Migration for SimpleMigration {
    fn name(&self) -> &'static str {
        self.name
    }

    fn graft_id(&self) -> GraftId {
        GraftId("stub")
    }

    fn target(&self) -> MigrationTarget {
        self.target
    }

    fn dependencies(&self) -> Vec<MigrationRef> {
        self.deps
            .iter()
            .map(|&name| MigrationRef {
                graft: GraftId("stub"),
                name,
            })
            .collect()
    }

    async fn up(&self, conn: &MigrationConn) -> Result<(), sea_orm::DbErr> {
        conn.execute_unprepared(&self.sql).await?;
        Ok(())
    }
}

fn _assert_graft_build_state_is_send() {
    fn require_send<T: Send>(_t: T) {}
    let graft = StubGraft::diamond();
    let tenancy = Tenancy::new(std::path::PathBuf::from("/tmp"));
    require_send(graft.build_state(&tenancy));
}
