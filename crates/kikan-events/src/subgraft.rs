use std::sync::Arc;

use kikan::error::EngineError;
use kikan::graft::SubGraft;
use kikan::migrations::{GraftId, Migration};

use crate::bus::BroadcastEventBus;
use crate::event::LifecycleEvent;

const GRAFT_ID: GraftId = GraftId::new("kikan-events");

pub struct EventBusSubGraft {
    bus: Arc<BroadcastEventBus>,
}

impl EventBusSubGraft {
    pub fn new(bus: Arc<BroadcastEventBus>) -> Self {
        Self { bus }
    }
}

#[async_trait::async_trait]
impl SubGraft for EventBusSubGraft {
    fn id(&self) -> GraftId {
        GRAFT_ID
    }

    fn migrations(&self) -> Vec<Box<dyn Migration>> {
        vec![]
    }

    async fn on_ignite(&self) -> Result<(), EngineError> {
        self.bus.publish_lifecycle(LifecycleEvent::BootStarted);
        Ok(())
    }

    async fn on_liftoff(&self) -> Result<(), EngineError> {
        self.bus.publish_lifecycle(LifecycleEvent::Serving);
        Ok(())
    }

    async fn on_shutdown(&self) -> Result<(), EngineError> {
        self.bus
            .publish_lifecycle(LifecycleEvent::ShutdownInitiated);
        self.bus.publish_lifecycle(LifecycleEvent::ShutdownComplete);
        Ok(())
    }
}
