use std::sync::Arc;

use kikan::error::EngineError;
use kikan::graft::SubGraft;
use kikan::migrations::{GraftId, Migration};

use crate::apalis_impl::ApalisScheduler;
use crate::scheduler::Scheduler;

const GRAFT_ID: GraftId = GraftId::new("kikan-scheduler");

pub struct SchedulerSubGraft {
    scheduler: Arc<ApalisScheduler>,
}

impl SchedulerSubGraft {
    pub fn new(scheduler: Arc<ApalisScheduler>) -> Self {
        Self { scheduler }
    }
}

#[async_trait::async_trait]
impl SubGraft for SchedulerSubGraft {
    fn id(&self) -> GraftId {
        GRAFT_ID
    }

    fn migrations(&self) -> Vec<Box<dyn Migration>> {
        vec![]
    }

    async fn on_shutdown(&self) -> Result<(), EngineError> {
        self.scheduler.shutdown_token().cancel();
        Ok(())
    }

    async fn check(&self) -> Result<(), EngineError> {
        self.scheduler
            .check()
            .await
            .map_err(|e| EngineError::Boot(e.to_string()))
    }
}
