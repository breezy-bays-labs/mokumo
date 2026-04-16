use std::marker::PhantomData;
use std::sync::Arc;

use crate::boot::BootConfig;
use crate::error::EngineError;
use crate::graft::{Graft, SelfGraft, SubGraft};
use crate::migrations;
use crate::migrations::Migration;
use crate::tenancy::Tenancy;

#[derive(Clone, Default)]
pub struct EngineContext;

pub struct Engine<G: Graft> {
    config: BootConfig,
    tenancy: Tenancy,
    all_migrations: Vec<Arc<dyn Migration>>,
    _graft: PhantomData<G>,
}

impl<G: Graft> Engine<G> {
    pub fn new(config: BootConfig, graft: &G) -> Result<Self, EngineError> {
        let tenancy = Tenancy::new(config.data_dir.clone());

        let subgraft_migrations: Vec<Vec<Box<dyn Migration>>> =
            std::iter::once(SelfGraft.migrations())
                .chain(config.subgrafts.iter().map(|sg| sg.migrations()))
                .collect();

        let all_migrations =
            migrations::collect_migrations(graft.migrations(), subgraft_migrations);

        Ok(Self {
            config,
            tenancy,
            all_migrations,
            _graft: PhantomData,
        })
    }

    pub async fn run_migrations(
        &self,
        pool: &sea_orm::DatabaseConnection,
    ) -> Result<(), EngineError> {
        migrations::runner::run_migrations_with_backfill(pool, &self.all_migrations, Some(G::id()))
            .await
    }

    pub fn tenancy(&self) -> &Tenancy {
        &self.tenancy
    }

    pub fn config(&self) -> &BootConfig {
        &self.config
    }

    pub fn context(&self) -> EngineContext {
        EngineContext
    }
}
