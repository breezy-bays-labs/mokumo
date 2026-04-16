use std::sync::Arc;
use tokio::sync::broadcast;

use crate::event::{HealthEvent, LifecycleEvent, MigrationEvent, ProfileEvent};

pub const DEFAULT_CAPACITY: usize = 1024;

pub struct BroadcastEventBus {
    lifecycle: broadcast::Sender<LifecycleEvent>,
    health: broadcast::Sender<HealthEvent>,
    migration: broadcast::Sender<MigrationEvent>,
    profile: broadcast::Sender<ProfileEvent>,
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
            lifecycle: broadcast::channel(cap).0,
            health: broadcast::channel(cap).0,
            migration: broadcast::channel(cap).0,
            profile: broadcast::channel(cap).0,
        })
    }

    pub fn publish_lifecycle(&self, e: LifecycleEvent) {
        let _ = self.lifecycle.send(e);
    }

    pub fn publish_health(&self, e: HealthEvent) {
        let _ = self.health.send(e);
    }

    pub fn publish_migration(&self, e: MigrationEvent) {
        let _ = self.migration.send(e);
    }

    pub fn publish_profile(&self, e: ProfileEvent) {
        let _ = self.profile.send(e);
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
