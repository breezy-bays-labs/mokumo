use std::sync::Arc;
use tokio::sync::broadcast;

use crate::channel::{DEFAULT_CAPACITY, FanoutChannel};
use crate::event::{HealthEvent, LifecycleEvent, MigrationEvent, ProfileEvent};

pub struct BroadcastEventBus {
    lifecycle: FanoutChannel<LifecycleEvent>,
    health: FanoutChannel<HealthEvent>,
    migration: FanoutChannel<MigrationEvent>,
    profile: FanoutChannel<ProfileEvent>,
}

impl std::fmt::Debug for BroadcastEventBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BroadcastEventBus").finish_non_exhaustive()
    }
}

impl BroadcastEventBus {
    pub fn new() -> Arc<Self> {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    pub fn with_capacity(cap: usize) -> Arc<Self> {
        Arc::new(Self {
            lifecycle: FanoutChannel::with_capacity(cap),
            health: FanoutChannel::with_capacity(cap),
            migration: FanoutChannel::with_capacity(cap),
            profile: FanoutChannel::with_capacity(cap),
        })
    }

    pub fn publish_lifecycle(&self, e: LifecycleEvent) {
        let _ = self.lifecycle.publish(e);
    }

    pub fn publish_health(&self, e: HealthEvent) {
        let _ = self.health.publish(e);
    }

    pub fn publish_migration(&self, e: MigrationEvent) {
        let _ = self.migration.publish(e);
    }

    pub fn publish_profile(&self, e: ProfileEvent) {
        let _ = self.profile.publish(e);
    }

    pub fn subscribe_lifecycle(&self) -> broadcast::Receiver<LifecycleEvent> {
        self.lifecycle.subscribe()
    }

    pub fn subscribe_health(&self) -> broadcast::Receiver<HealthEvent> {
        self.health.subscribe()
    }

    pub fn subscribe_migration(&self) -> broadcast::Receiver<MigrationEvent> {
        self.migration.subscribe()
    }

    pub fn subscribe_profile(&self) -> broadcast::Receiver<ProfileEvent> {
        self.profile.subscribe()
    }
}
